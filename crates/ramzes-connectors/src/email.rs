use std::net::IpAddr;
use std::time::Duration;

use async_trait::async_trait;
use lettre::message::{Mailbox, MultiPart, header::ContentType};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};
use ramzes_core::{EventStatus, Message};

use crate::http::{PRIVATE_REFUSED, is_private};
use crate::{
    Capabilities, ConnectionConfig, Connector, ConnectorDescriptor, Field, FieldKind, HttpOptions,
    Preview, SendOutcome, truncate,
};

const SUBJECT_MAX: usize = 200;

/// Sends each message as an email over SMTP.
pub(crate) struct Email {
    timeout: Duration,
    block_private: bool,
}

impl Email {
    pub fn new(options: HttpOptions) -> Email {
        // lettre builds its TLS settings from the process default; make sure there is one.
        let _ = rustls::crypto::ring::default_provider().install_default();
        Email {
            timeout: options.timeout,
            block_private: options.block_private,
        }
    }

    /// Where to connect. With private addresses blocked this is one public address the name
    /// resolves to, so the server cannot be swapped for a private one between check and use.
    async fn server(&self, host: &str, port: u16) -> Result<String, SendOutcome> {
        if !self.block_private {
            return Ok(host.to_owned());
        }
        let found: Vec<IpAddr> = match host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => match tokio::net::lookup_host((host, port)).await {
                Ok(found) => found.map(|addr| addr.ip()).collect(),
                Err(e) => {
                    return Err(SendOutcome::Retry {
                        after_ms: None,
                        status: None,
                        detail: format!("the mail server's name could not be resolved: {e}"),
                    });
                }
            },
        };
        match found.into_iter().find(|ip| !is_private(*ip)) {
            Some(ip) => Ok(ip.to_string()),
            None => Err(permanent(PRIVATE_REFUSED)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Security {
    /// Upgrade a plain connection with STARTTLS; refuse to send without it.
    StartTls,
    /// TLS from the first byte (port 465).
    Tls,
    /// No encryption. For a relay on a trusted network.
    None,
}

struct Settings<'a> {
    host: &'a str,
    port: u16,
    security: Security,
    from: Mailbox,
    username: Option<&'a str>,
}

fn settings(c: &ConnectionConfig) -> Result<Settings<'_>, String> {
    let host = c.setting("host").ok_or("Server is required")?;
    let security = match c.setting("security").unwrap_or("starttls") {
        "starttls" => Security::StartTls,
        "tls" => Security::Tls,
        "none" => Security::None,
        _ => return Err("Security must be starttls, tls or none".into()),
    };
    let default_port = match security {
        Security::StartTls => 587,
        Security::Tls => 465,
        Security::None => 25,
    };
    let port = match c.setting("port") {
        None => default_port,
        Some(text) => text
            .trim()
            .parse::<u16>()
            .ok()
            .filter(|port| *port != 0)
            .ok_or("Port must be a number from 1 to 65535")?,
    };
    let from = c
        .setting("from")
        .ok_or("From is required")?
        .parse::<Mailbox>()
        .map_err(|_| "From must be an email address, for example ramzes <alerts@example.com>")?;
    Ok(Settings {
        host,
        port,
        security,
        from,
        username: c.setting("username"),
    })
}

/// The recipients: the route's destination, or the connection's default.
fn recipients(c: &ConnectionConfig, destination: Option<&str>) -> Result<Vec<Mailbox>, String> {
    let list = destination
        .filter(|d| !d.trim().is_empty())
        .or_else(|| c.setting("to"))
        .ok_or("there is no recipient: set one on the route or on the connection")?;
    let mut out = Vec::new();
    for part in list
        .split([',', ';'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        out.push(
            part.parse::<Mailbox>()
                .map_err(|_| format!("{part:?} is not an email address"))?,
        );
    }
    if out.is_empty() {
        return Err("there is no recipient: set one on the route or on the connection".into());
    }
    Ok(out)
}

/// One line, no control characters: a subject must not be able to start a new header.
fn subject(m: &Message) -> String {
    let marker = match m.status {
        EventStatus::Resolved => "resolved".to_owned(),
        EventStatus::Firing => serde_json::to_value(m.severity)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
    };
    let title: String = m
        .title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    truncate(
        &format!(
            "[{marker}] {}",
            title.split_whitespace().collect::<Vec<_>>().join(" ")
        ),
        SUBJECT_MAX,
    )
}

fn plain(m: &Message) -> String {
    let mut out = String::new();
    if let Some(body) = &m.body {
        out.push_str(body);
        out.push_str("\n\n");
    }
    for (key, value) in &m.fields {
        out.push_str(&format!("{key}: {value}\n"));
    }
    out.push_str(&format!("\ntopic: {}\n", m.topic));
    if let Some(link) = &m.link {
        out.push_str(&format!("Open in ramzes: {link}\n"));
    }
    if let Some(link) = &m.ack_link {
        out.push_str(&format!("Acknowledge: {link}\n"));
    }
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html(m: &Message) -> String {
    let mut out = String::from(
        "<div style=\"font-family:ui-monospace,Menlo,Consolas,monospace;font-size:14px;line-height:1.5\">",
    );
    out.push_str(&format!(
        "<p style=\"font-size:16px;font-weight:600;margin:0 0 12px\">{}</p>",
        escape(&m.title)
    ));
    if let Some(body) = &m.body {
        out.push_str(&format!(
            "<p style=\"white-space:pre-wrap;margin:0 0 12px\">{}</p>",
            escape(body)
        ));
    }
    if !m.fields.is_empty() {
        out.push_str("<table style=\"border-collapse:collapse;margin:0 0 12px\">");
        for (key, value) in &m.fields {
            out.push_str(&format!(
                "<tr><td style=\"padding:2px 16px 2px 0;color:#666\">{}</td><td style=\"padding:2px 0\">{}</td></tr>",
                escape(key),
                escape(value)
            ));
        }
        out.push_str("</table>");
    }
    out.push_str(&format!(
        "<p style=\"color:#666;margin:0\">{}",
        escape(&m.topic)
    ));
    if let Some(link) = &m.link {
        out.push_str(&format!(
            " · <a href=\"{}\">Open in ramzes</a>",
            escape(link)
        ));
    }
    if let Some(link) = &m.ack_link {
        out.push_str(&format!(" · <a href=\"{}\">Acknowledge</a>", escape(link)));
    }
    out.push_str("</p></div>");
    out
}

fn permanent(detail: impl Into<String>) -> SendOutcome {
    SendOutcome::Permanent {
        status: None,
        detail: detail.into(),
    }
}

fn field(key: &str, label: &str, help: &str, secret: bool, required: bool) -> Field {
    Field {
        key: key.into(),
        label: label.into(),
        help: help.into(),
        secret,
        required,
        kind: FieldKind::Text,
    }
}

#[async_trait]
impl Connector for Email {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: "email".into(),
            name: "Email".into(),
            description: "Send each message as an email through an SMTP server.".into(),
            fields: vec![
                field(
                    "host",
                    "Server",
                    "The SMTP server's host name.",
                    false,
                    true,
                ),
                field(
                    "port",
                    "Port",
                    "Leave empty for 587 with STARTTLS, 465 with TLS, 25 without encryption.",
                    false,
                    false,
                ),
                field(
                    "security",
                    "Security",
                    "starttls (default), tls or none.",
                    false,
                    false,
                ),
                field(
                    "username",
                    "User name",
                    "Leave empty when the server needs no sign-in.",
                    false,
                    false,
                ),
                field("password", "Password", "", true, false),
                field(
                    "from",
                    "From",
                    "For example: ramzes <alerts@example.com>",
                    false,
                    true,
                ),
                field(
                    "to",
                    "Default recipients",
                    "Used by routes that name no recipient. Separate several with commas.",
                    false,
                    false,
                ),
            ],
            capabilities: Capabilities {
                rich: true,
                update: false,
                threads: false,
                buttons: false,
                max_len: 20_000,
            },
            destination_label: Some("Recipients".into()),
            custom: false,
        }
    }

    fn validate(&self, c: &ConnectionConfig) -> Result<(), String> {
        settings(c)?;
        if c.setting("to").is_some() {
            recipients(c, None)?;
        }
        Ok(())
    }

    fn preview(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        _now_ms: i64,
    ) -> Result<Preview, String> {
        let s = settings(c)?;
        let to = recipients(c, destination)?
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        Ok(Preview {
            method: "SMTP".into(),
            url: format!("{}:{}", s.host, s.port),
            headers: vec![
                ("From".to_owned(), s.from.to_string()),
                ("To".to_owned(), to),
                ("Subject".to_owned(), subject(m)),
            ],
            body: plain(m),
        })
    }

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        _now_ms: i64,
    ) -> SendOutcome {
        let s = match settings(c) {
            Ok(s) => s,
            Err(e) => return permanent(e),
        };
        let to = match recipients(c, destination) {
            Ok(to) => to,
            Err(e) => return permanent(e),
        };
        let mut builder = lettre::Message::builder()
            .from(s.from.clone())
            .subject(subject(m));
        for mailbox in to {
            builder = builder.to(mailbox);
        }
        let email = match builder.multipart(
            MultiPart::alternative()
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_PLAIN)
                        .body(plain(m)),
                )
                .singlepart(
                    lettre::message::SinglePart::builder()
                        .header(ContentType::TEXT_HTML)
                        .body(html(m)),
                ),
        ) {
            Ok(email) => email,
            Err(e) => return permanent(format!("the email could not be built: {e}")),
        };

        let tls = match s.security {
            Security::None => Tls::None,
            Security::StartTls | Security::Tls => match TlsParameters::new(s.host.to_owned()) {
                Ok(parameters) if s.security == Security::Tls => Tls::Wrapper(parameters),
                Ok(parameters) => Tls::Required(parameters),
                Err(e) => return permanent(format!("TLS could not be set up: {e}")),
            },
        };
        let server = match self.server(s.host, s.port).await {
            Ok(server) => server,
            Err(outcome) => return outcome,
        };
        let mut transport = AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(server)
            .port(s.port)
            .tls(tls)
            .timeout(Some(self.timeout));
        if let Some(username) = s.username {
            let password = c.credential("password").unwrap_or_default();
            transport =
                transport.credentials(Credentials::new(username.to_owned(), password.to_owned()));
        }
        match transport.build().send(email).await {
            Ok(response) => SendOutcome::Delivered {
                external_ref: None,
                status: Some(response.code().into()),
            },
            Err(e) => {
                let detail = truncate(&format!("the mail server said: {e}"), 500);
                let status = e.status().map(u16::from);
                if e.is_permanent() {
                    SendOutcome::Permanent { status, detail }
                } else {
                    SendOutcome::Retry {
                        after_ms: None,
                        status,
                        detail,
                    }
                }
            }
        }
    }
}
