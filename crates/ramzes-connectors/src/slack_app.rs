use async_trait::async_trait;
use ramzes_core::{EventStatus, Message};
use serde_json::{Value, json};

use crate::http::{Http, Outgoing};
use crate::slack::{blocks, fallback};
use crate::{
    Capabilities, ConnectionConfig, Connector, ConnectorDescriptor, Field, FieldKind, Preview,
    SendOutcome, mask_secrets, require_http_url,
};

const DEFAULT_API: &str = "https://slack.com/api";
/// The `action_id` of the button that acknowledges a notification.
pub const ACK_ACTION: &str = "ramzes_ack";
/// The `action_id` of the button that silences events like the one shown for an hour.
pub const SILENCE_ACTION: &str = "ramzes_silence";

/// Errors Slack reports in a 200 answer that are worth another attempt.
const TRANSIENT: [&str; 6] = [
    "rate_limited",
    "ratelimited",
    "service_unavailable",
    "internal_error",
    "fatal_error",
    "request_timeout",
];

/// Posts through a Slack app's bot token: messages carry buttons, follow-ups go into the
/// thread of the first message, and a message is rewritten when its notification is
/// acknowledged or resolved.
pub(crate) struct SlackApp {
    http: Http,
}

impl SlackApp {
    pub fn new(http: Http) -> SlackApp {
        SlackApp { http }
    }

    fn api(c: &ConnectionConfig) -> String {
        c.setting("api_url")
            .unwrap_or(DEFAULT_API)
            .trim_end_matches('/')
            .to_owned()
    }

    fn channel<'a>(c: &'a ConnectionConfig, destination: Option<&'a str>) -> Option<&'a str> {
        destination
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .or_else(|| c.setting("channel"))
    }

    /// Calls a Web API method and returns its JSON answer, or the outcome a failure amounts to.
    async fn call(
        &self,
        c: &ConnectionConfig,
        method: &str,
        body: &Value,
    ) -> Result<Value, SendOutcome> {
        let Some(token) = c.credential("bot_token") else {
            return Err(SendOutcome::Permanent {
                status: None,
                detail: "the connection has no bot token".into(),
            });
        };
        let url = format!("{}/{method}", Self::api(c));
        let headers = [
            (
                "Content-Type".to_owned(),
                "application/json; charset=utf-8".to_owned(),
            ),
            ("Authorization".to_owned(), format!("Bearer {token}")),
        ];
        let answer = self
            .http
            .call(&Outgoing {
                method: "POST",
                url: &url,
                headers: &headers,
                body: body.to_string().into_bytes(),
                success: &[],
                retry_after_header: "retry-after",
            })
            .await?;
        if answer.status != 200 {
            return Err(answer.classified());
        }
        let parsed: Value = serde_json::from_str(&answer.body).map_err(|_| SendOutcome::Retry {
            after_ms: None,
            status: Some(200),
            detail: "Slack answered with something that is not JSON".into(),
        })?;
        if parsed["ok"].as_bool() == Some(true) {
            return Ok(parsed);
        }
        let error = parsed["error"]
            .as_str()
            .unwrap_or("unknown_error")
            .to_owned();
        let hint = match error.as_str() {
            "not_in_channel" => ": invite the app to the channel",
            "channel_not_found" => {
                ": check the channel ID, and invite the app if the channel is private"
            }
            "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" => {
                ": check the bot token"
            }
            "missing_scope" => ": the app needs the chat:write scope",
            _ => "",
        };
        let detail = format!("Slack said {error}{hint}");
        Err(if TRANSIENT.contains(&error.as_str()) {
            SendOutcome::Retry {
                after_ms: None,
                status: Some(200),
                detail,
            }
        } else {
            SendOutcome::Permanent {
                status: Some(200),
                detail,
            }
        })
    }
}

/// The buttons under a firing message. Their value is the notification's id.
fn actions(m: &Message) -> Option<Value> {
    let id = m.notification_id.as_deref()?;
    if m.status == EventStatus::Resolved {
        return None;
    }
    let mut elements = vec![
        json!({"type": "button", "action_id": ACK_ACTION, "value": id, "style": "primary",
               "text": {"type": "plain_text", "text": "Acknowledge"}}),
        json!({"type": "button", "action_id": SILENCE_ACTION, "value": id,
               "text": {"type": "plain_text", "text": "Silence for 1 hour"}}),
    ];
    if let Some(link) = &m.link {
        elements.push(
            json!({"type": "button", "action_id": "ramzes_open", "url": link,
                             "text": {"type": "plain_text", "text": "Open in ramzes"}}),
        );
    }
    Some(json!({"type": "actions", "elements": elements}))
}

fn post_body(m: &Message, channel: &str) -> Value {
    let mut all = blocks(m, false);
    all.extend(actions(m));
    let mut body = json!({"channel": channel, "text": fallback(m), "blocks": all});
    // A follow-up goes under the first message, when that one is in the same channel.
    if let Some((first_channel, ts)) = m.thread_ref.as_deref().and_then(|r| r.split_once(':'))
        && first_channel == channel
    {
        body["thread_ts"] = ts.into();
    }
    body
}

fn field(
    key: &str,
    label: &str,
    help: &str,
    secret: bool,
    required: bool,
    kind: FieldKind,
) -> Field {
    Field {
        key: key.into(),
        label: label.into(),
        help: help.into(),
        secret,
        required,
        kind,
    }
}

#[async_trait]
impl Connector for SlackApp {
    fn descriptor(&self) -> ConnectorDescriptor {
        ConnectorDescriptor {
            kind: "slack_app".into(),
            name: "Slack app".into(),
            description: "Post through your own Slack app: messages get Acknowledge and Silence buttons, follow-ups are threaded, and a message is updated when it is acknowledged or resolved.".into(),
            fields: vec![
                field("bot_token", "Bot token", "Starts with xoxb-. The app needs the chat:write scope and must be in the channel.", true, true, FieldKind::Text),
                field("signing_secret", "Signing secret", "From the app's Basic Information page. Needed for the buttons: ramzes checks that button presses come from Slack.", true, false, FieldKind::Text),
                field("channel", "Default channel", "A channel ID such as C0123456789. Used by routes that name no channel.", false, false, FieldKind::Text),
                field("api_url", "API address", "Leave empty for https://slack.com/api.", false, false, FieldKind::Url),
            ],
            capabilities: Capabilities {
                rich: true,
                update: true,
                threads: true,
                buttons: true,
                max_len: 3000,
            },
            destination_label: Some("Channel ID".into()),
            custom: false,
        }
    }

    fn validate(&self, c: &ConnectionConfig) -> Result<(), String> {
        if c.credential("bot_token").is_none() {
            return Err("Bot token is required".into());
        }
        if c.setting("api_url").is_some() {
            require_http_url("API address", c.setting("api_url"))?;
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
        let channel = Self::channel(c, destination)
            .ok_or("there is no channel: set one on the route or on the connection")?;
        let body =
            serde_json::to_string_pretty(&post_body(m, channel)).map_err(|e| e.to_string())?;
        Ok(mask_secrets(
            Preview {
                method: "POST".into(),
                url: format!("{}/chat.postMessage", Self::api(c)),
                headers: vec![
                    (
                        "Content-Type".to_owned(),
                        "application/json; charset=utf-8".to_owned(),
                    ),
                    (
                        "Authorization".to_owned(),
                        format!("Bearer {}", c.credential("bot_token").unwrap_or_default()),
                    ),
                ],
                body,
            },
            &c.credentials,
        ))
    }

    async fn send(
        &self,
        c: &ConnectionConfig,
        m: &Message,
        destination: Option<&str>,
        _now_ms: i64,
    ) -> SendOutcome {
        let Some(channel) = Self::channel(c, destination) else {
            return SendOutcome::Permanent {
                status: None,
                detail: "there is no channel: set one on the route or on the connection".into(),
            };
        };
        match self
            .call(c, "chat.postMessage", &post_body(m, channel))
            .await
        {
            Ok(answer) => {
                // Slack may answer with the channel's canonical id; keep that with the timestamp.
                let posted_in = answer["channel"].as_str().unwrap_or(channel);
                SendOutcome::Delivered {
                    external_ref: answer["ts"].as_str().map(|ts| format!("{posted_in}:{ts}")),
                    status: Some(200),
                }
            }
            Err(outcome) => outcome,
        }
    }

    async fn update(
        &self,
        c: &ConnectionConfig,
        external_ref: &str,
        m: &Message,
        note: &str,
    ) -> Result<(), String> {
        let (channel, ts) = external_ref
            .split_once(':')
            .ok_or("the message reference is not one of Slack's")?;
        let mut all = blocks(m, false);
        all.push(json!({"type": "context", "elements": [{"type": "plain_text", "text": note, "emoji": true}]}));
        let body = json!({"channel": channel, "ts": ts, "text": fallback(m), "blocks": all});
        match self.call(c, "chat.update", &body).await {
            Ok(_) => Ok(()),
            Err(SendOutcome::Retry { detail, .. } | SendOutcome::Permanent { detail, .. }) => {
                Err(detail)
            }
            Err(SendOutcome::Delivered { .. }) => Ok(()),
        }
    }
}
