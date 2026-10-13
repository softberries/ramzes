use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use crate::{SendOutcome, truncate};

const EXCERPT_CHARS: usize = 500;

/// How connectors talk to services.
#[derive(Debug, Clone, Copy)]
pub struct HttpOptions {
    /// Bounds each request to a service.
    pub timeout: Duration,
    /// Refuse to send to loopback, private and link-local addresses.
    pub block_private: bool,
}

/// Maps an HTTP response to an outcome: 2xx delivered; 408, 425, 429 and 5xx retry; anything else is final.
pub fn classify(status: u16, retry_after: Option<&str>, body_excerpt: String) -> SendOutcome {
    let detail = if body_excerpt.is_empty() {
        format!("HTTP {status}")
    } else {
        format!("HTTP {status}: {body_excerpt}")
    };
    match status {
        200..=299 => SendOutcome::Delivered {
            external_ref: None,
            status: Some(status),
        },
        408 | 425 | 429 | 500..=599 => SendOutcome::Retry {
            after_ms: retry_after
                .and_then(|s| s.trim().parse::<f64>().ok())
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .map(|seconds| (seconds * 1000.0).min(86_400_000.0) as i64),
            status: Some(status),
            detail,
        },
        _ => SendOutcome::Permanent {
            status: Some(status),
            detail,
        },
    }
}

/// Addresses that only make sense from inside the network ramzes runs in.
pub(crate) fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                // "This network" 0.0.0.0/8, and the reserved and broadcast block 240.0.0.0/4.
                || a == 0
                || a >= 240
                // Carrier-grade NAT, 100.64.0.0/10.
                || (a == 100 && (64..128).contains(&b))
                // Benchmarking, 198.18.0.0/15.
                || (a == 198 && (18..20).contains(&b))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_private(IpAddr::V4(v4));
            }
            let segments = v6.segments();
            // An IPv4 address carried in IPv6: NAT64 64:ff9b::/96 and the old ::a.b.c.d form.
            let carries_v4 =
                (segments[0] == 0x64 && segments[1] == 0xff9b && segments[2..6] == [0; 4])
                    || (segments[..6] == [0; 6] && !v6.is_loopback() && !v6.is_unspecified());
            if carries_v4 {
                let [.., hi, lo] = segments;
                let v4 =
                    std::net::Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
                return is_private(IpAddr::V4(v4));
            }
            let first = segments[0];
            v6.is_loopback()
                || v6.is_unspecified()
                // Unique local fc00::/7 and link-local fe80::/10.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80
        }
    }
}

/// Resolves names and drops private addresses, so a name cannot point a request inwards.
struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let found = tokio::net::lookup_host((name.as_str(), 0)).await?;
            let public: Vec<SocketAddr> = found.filter(|addr| !is_private(addr.ip())).collect();
            if public.is_empty() {
                return Err("the name resolves only to private addresses".into());
            }
            let addrs: Addrs = Box::new(public.into_iter());
            Ok(addrs)
        })
    }
}

/// One request to a service.
pub(crate) struct Outgoing<'a> {
    pub method: &'a str,
    pub url: &'a str,
    pub headers: &'a [(String, String)],
    pub body: Vec<u8>,
    /// Statuses that mean delivered. Empty means any 2xx.
    pub success: &'a [u16],
    /// The response header that says how many seconds to wait before retrying.
    pub retry_after_header: &'a str,
}

/// Most of an answer that is read. Connectors need at most a short JSON document or an excerpt.
const ANSWER_MAX: usize = 64 * 1024;

/// Reads the answer's body up to [`ANSWER_MAX`] bytes and leaves the rest unread.
async fn read_limited(mut response: reqwest::Response) -> Vec<u8> {
    let mut body = Vec::new();
    while let Ok(Some(chunk)) = response.chunk().await {
        let room = ANSWER_MAX - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(room)]);
        if body.len() >= ANSWER_MAX {
            break;
        }
    }
    body
}

/// What a service answered.
pub(crate) struct Answer {
    pub status: u16,
    pub retry_after: Option<String>,
    pub body: String,
}

impl Answer {
    /// The outcome the status alone amounts to.
    pub fn classified(&self) -> SendOutcome {
        classify(
            self.status,
            self.retry_after.as_deref(),
            truncate(self.body.trim(), EXCERPT_CHARS),
        )
    }
}

/// The HTTP client shared by connectors.
#[derive(Clone)]
pub(crate) struct Http {
    client: reqwest::Client,
    block_private: bool,
}

const PRIVATE_REFUSED: &str =
    "not sent: the address is private, and this instance blocks requests to private addresses";

impl Http {
    pub fn new(options: HttpOptions) -> Http {
        let mut builder = reqwest::Client::builder()
            .timeout(options.timeout)
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("ramzes/", env!("CARGO_PKG_VERSION")));
        if options.block_private {
            builder = builder.dns_resolver(Arc::new(PublicOnly));
        }
        Http {
            client: builder
                .build()
                .expect("the HTTP client configuration is static and valid"),
            block_private: options.block_private,
        }
    }

    /// Whether the URL's host is, or resolves to, a private address.
    async fn points_inwards(url: &reqwest::Url) -> bool {
        match url.host() {
            Some(url::Host::Ipv4(ip)) => is_private(IpAddr::V4(ip)),
            Some(url::Host::Ipv6(ip)) => is_private(IpAddr::V6(ip)),
            Some(url::Host::Domain(name)) => {
                match tokio::net::lookup_host((name, url.port_or_known_default().unwrap_or(443)))
                    .await
                {
                    Ok(mut found) => found.any(|addr| is_private(addr.ip())),
                    // An unresolvable name fails later as a connection error, which is retried.
                    Err(_) => false,
                }
            }
            None => true,
        }
    }

    /// Sends the request and returns the answer as it came. A request that could not be made
    /// is returned as the outcome it amounts to: network failures and timeouts are retryable.
    pub async fn call(&self, out: &Outgoing<'_>) -> Result<Answer, SendOutcome> {
        let permanent = |detail: String| SendOutcome::Permanent {
            status: None,
            detail,
        };
        let url = match reqwest::Url::parse(out.url) {
            Ok(url) if matches!(url.scheme(), "http" | "https") && url.has_host() => url,
            _ => return Err(permanent("the address is not an http or https URL".into())),
        };
        let Ok(method) = reqwest::Method::from_bytes(out.method.as_bytes()) else {
            return Err(permanent(format!("{:?} is not an HTTP method", out.method)));
        };
        if self.block_private && Self::points_inwards(&url).await {
            return Err(permanent(PRIVATE_REFUSED.into()));
        }
        let mut request = self.client.request(method, url).body(out.body.clone());
        for (name, value) in out.headers {
            request = request.header(name.as_str(), value.as_str());
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(e) => {
                let detail = if e.is_timeout() {
                    "the request timed out".to_owned()
                } else if e.is_connect() {
                    "could not connect to the service".to_owned()
                } else if e.is_builder() {
                    return Err(permanent(format!(
                        "the request could not be built: {}",
                        e.without_url()
                    )));
                } else {
                    format!("the request failed: {}", e.without_url())
                };
                return Err(SendOutcome::Retry {
                    after_ms: None,
                    status: None,
                    detail,
                });
            }
        };
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get(out.retry_after_header)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        // A NUL cannot be stored as text in Postgres; binary answers are kept without them.
        let body = String::from_utf8_lossy(&read_limited(response).await).replace('\0', "");
        Ok(Answer {
            status,
            retry_after,
            body,
        })
    }

    /// Sends the request and classifies the answer by its status.
    pub async fn send(&self, out: Outgoing<'_>) -> SendOutcome {
        let answer = match self.call(&out).await {
            Ok(answer) => answer,
            Err(outcome) => return outcome,
        };
        let status = answer.status;
        if !out.success.is_empty() && (200..300).contains(&status) && !out.success.contains(&status)
        {
            return SendOutcome::Permanent {
                status: Some(status),
                detail: format!("HTTP {status}, which this connector does not count as delivered"),
            };
        }
        if out.success.contains(&status) {
            return SendOutcome::Delivered {
                external_ref: None,
                status: Some(status),
            };
        }
        answer.classified()
    }

    /// Posts a JSON body; any 2xx counts as delivered.
    pub async fn post_json(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: Vec<u8>,
    ) -> SendOutcome {
        let mut all = vec![("content-type".to_owned(), "application/json".to_owned())];
        all.extend_from_slice(headers);
        self.send(Outgoing {
            method: "POST",
            url,
            headers: &all,
            body,
            success: &[],
            retry_after_header: "retry-after",
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::is_private;

    #[test]
    fn inward_addresses_are_recognised_in_every_spelling() {
        for inward in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "0.1.2.3",
            "100.64.0.1",
            "198.18.0.1",
            "240.0.0.1",
            "255.255.255.255",
            "::1",
            "::",
            "fc00::1",
            "fe80::1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "::10.0.0.1",
        ] {
            assert!(is_private(inward.parse().unwrap()), "{inward}");
        }
        for outward in [
            "1.1.1.1",
            "93.184.216.34",
            "2606:4700:4700::1111",
            "64:ff9b::101:101",
            "198.20.0.1",
        ] {
            assert!(!is_private(outward.parse().unwrap()), "{outward}");
        }
    }
}
