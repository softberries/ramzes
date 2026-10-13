//! Pages and endpoints reached without signing in: the acknowledgement link in messages and
//! the address Slack sends button presses to. Each proves itself by a signature instead.

use axum::Router;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use hmac::{Hmac, Mac};
use ramzes_connectors::{ACK_ACTION, SILENCE_ACTION};
use ramzes_core::shaping::{NotificationState, Silence};
use ramzes_core::{LabelCond, RouteMatch, new_id};
use ramzes_engine::{Opener, ack_text};
use ramzes_store::StoreError;
use serde::Deserialize;
use sha2::Sha256;
use subtle::ConstantTimeEq;

use crate::AppState;

const HOUR_MS: i64 = 3_600_000;
/// How far a Slack request's timestamp may be from now.
const SLACK_SKEW_S: i64 = 300;
/// Labels copied into a silence made from a Slack button.
const SILENCE_LABELS: usize = 10;

pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/ack/{id}", get(ack_page).post(ack_submit))
        .route(
            "/v1/slack/{connection_id}/interactions",
            post(slack_interaction),
        )
        .with_state(state)
}

#[derive(Deserialize)]
struct Token {
    #[serde(default)]
    t: String,
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// A small page that reads well on a phone, in the colours of the UI's dark theme.
fn page(status: StatusCode, heading: &str, body: &str) -> Response {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
         <meta name=\"robots\" content=\"noindex\"><title>{heading} · ramzes</title><style>\
         body{{margin:0;background:#0b1020;color:#e6e9f2;font:15px/1.5 ui-monospace,SFMono-Regular,Menlo,Consolas,monospace}}\
         main{{max-width:34rem;margin:0 auto;padding:3rem 1.25rem}}h1{{font-size:1.1rem;margin:0 0 1rem}}\
         p{{margin:0 0 1rem;color:#aab2c8}}.what{{color:#e6e9f2;border-left:2px solid #d9a441;padding-left:.75rem}}\
         button{{font:inherit;font-weight:600;color:#0b1020;background:#8fb0ff;border:0;border-radius:2px;\
         padding:.6rem 1.1rem;cursor:pointer}}button:focus-visible{{outline:2px solid #d9a441;outline-offset:2px}}\
         </style></head><body><main><h1>{heading}</h1>{body}</main></body></html>",
        heading = escape(heading),
    );
    (status, Html(html)).into_response()
}

/// The same answer for a wrong token and for a notification that does not exist, so the page
/// says nothing about what exists.
fn not_valid() -> Response {
    page(
        StatusCode::NOT_FOUND,
        "This link is not valid",
        "<p>It may have been copied incompletely, or the notification is no longer kept. \
         Open ramzes to see what is waiting.</p>",
    )
}

fn token_matches(state: &AppState, id: &str, token: &str) -> bool {
    let expected = state.key.sign(&ack_text(id));
    bool::from(token.as_bytes().ct_eq(expected.as_bytes()))
}

async fn ack_page(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(token): Query<Token>,
) -> Response {
    if !token_matches(&state, &id, &token.t) {
        return not_valid();
    }
    let Ok(Some(detail)) = state.store.get_notification(&state.workspace_id, &id).await else {
        return not_valid();
    };
    let what = format!("<p class=\"what\">{}</p>", escape(&detail.row.title));
    match detail.row.state {
        NotificationState::Sent | NotificationState::Grouping | NotificationState::Held => page(
            StatusCode::OK,
            "Acknowledge this notification?",
            &format!(
                "{what}<p>Acknowledging stops reminders and escalation. Others see that someone is on it.</p>\
                 <form method=\"post\" action=\"/ack/{id}\"><input type=\"hidden\" name=\"t\" value=\"{t}\">\
                 <button type=\"submit\">Acknowledge</button></form>",
                id = escape(&id),
                t = escape(&token.t),
            ),
        ),
        NotificationState::Acknowledged => page(
            StatusCode::OK,
            "Already acknowledged",
            &format!("{what}<p>Someone has acknowledged this. Nothing more to do here.</p>"),
        ),
        NotificationState::Resolved | NotificationState::Suppressed => page(
            StatusCode::OK,
            "This is over",
            &format!("{what}<p>The notification is resolved. Nothing more to do here.</p>"),
        ),
    }
}

async fn ack_submit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Bytes,
) -> Response {
    let token: Token = serde_urlencoded::from_bytes(&body).unwrap_or(Token { t: String::new() });
    if !token_matches(&state, &id, &token.t) {
        return not_valid();
    }
    let now = state.clock.now_ms();
    match state
        .store
        .acknowledge(&state.workspace_id, &id, "link", now)
        .await
    {
        Ok(()) => {
            state.engine.notification_changed(&id);
            state.engine.request_refresh(&id);
            page(
                StatusCode::OK,
                "Acknowledged",
                "<p>Reminders and escalation for this notification have stopped. You can close this page.</p>",
            )
        }
        Err(StoreError::Conflict(_)) => page(
            StatusCode::OK,
            "Nothing to acknowledge",
            "<p>This notification was already acknowledged or is resolved.</p>",
        ),
        Err(StoreError::NotFound) => not_valid(),
        Err(e) => {
            tracing::error!(error = %e, "could not acknowledge from a link");
            page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "That did not work",
                "<p>ramzes could not record the acknowledgement. Try again in a moment.</p>",
            )
        }
    }
}

#[derive(Deserialize)]
struct SlackForm {
    payload: String,
}

#[derive(Deserialize)]
struct SlackPayload {
    #[serde(default)]
    user: SlackUser,
    #[serde(default)]
    actions: Vec<SlackAction>,
}

#[derive(Deserialize, Default)]
struct SlackUser {
    #[serde(default)]
    username: String,
    #[serde(default)]
    id: String,
}

#[derive(Deserialize)]
struct SlackAction {
    action_id: String,
    #[serde(default)]
    value: String,
}

/// Checks Slack's `v0` signature over the timestamp and the raw body with the app's signing secret.
async fn from_slack(
    state: &AppState,
    connection_id: &str,
    headers: &HeaderMap,
    body: &[u8],
) -> bool {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let (Some(timestamp), Some(signature)) = (
        header("x-slack-request-timestamp"),
        header("x-slack-signature"),
    ) else {
        return false;
    };
    let fresh = timestamp
        .parse::<i64>()
        .is_ok_and(|at| (state.clock.now_ms() / 1000 - at).abs() <= SLACK_SKEW_S);
    let Ok(Some(connection)) = state
        .store
        .get_connection(&state.workspace_id, connection_id)
        .await
    else {
        return false;
    };
    let secret = state
        .key
        .open(&connection.credentials)
        .ok()
        .and_then(|c| {
            c.get("signing_secret")
                .and_then(|s| s.as_str())
                .map(str::to_owned)
        })
        .filter(|s| !s.is_empty());
    let (true, "slack_app", Some(secret)) = (fresh, connection.kind.as_str(), secret) else {
        return false;
    };
    let Ok(mut mac) = <Hmac<Sha256> as hmac::KeyInit>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(format!("v0:{timestamp}:").as_bytes());
    mac.update(body);
    let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
    bool::from(signature.as_bytes().ct_eq(expected.as_bytes()))
}

/// Slack posts here when someone presses a button under a message. It must be answered within
/// three seconds; the message itself is rewritten afterwards by the delivery worker.
async fn slack_interaction(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if !from_slack(&state, &connection_id, &headers, &body).await {
        return StatusCode::UNAUTHORIZED;
    }
    let payload = serde_urlencoded::from_bytes::<SlackForm>(&body)
        .ok()
        .and_then(|form| serde_json::from_str::<SlackPayload>(&form.payload).ok());
    let Some(payload) = payload else {
        return StatusCode::BAD_REQUEST;
    };
    let name = if payload.user.username.is_empty() {
        &payload.user.id
    } else {
        &payload.user.username
    };
    let who = format!("{name} (Slack)");
    let ws = &state.workspace_id;
    let now = state.clock.now_ms();
    for action in &payload.actions {
        let id = &action.value;
        if action.action_id == ACK_ACTION {
            match state.store.acknowledge(ws, id, &who, now).await {
                Ok(()) => {
                    state.engine.notification_changed(id);
                    state.engine.request_refresh(id);
                }
                // Already acknowledged or gone: pressing again is harmless.
                Err(StoreError::Conflict(_) | StoreError::NotFound) => {}
                Err(e) => {
                    tracing::error!(error = %e, "could not acknowledge from Slack");
                    return StatusCode::INTERNAL_SERVER_ERROR;
                }
            }
        } else if action.action_id == SILENCE_ACTION {
            let Ok(Some(detail)) = state.store.get_notification(ws, id).await else {
                continue;
            };
            let Some(first) = detail.events.first() else {
                continue;
            };
            let silence = Silence {
                id: new_id(),
                r#match: RouteMatch {
                    topic: Some(first.event.topic.clone()),
                    labels: first
                        .event
                        .labels
                        .iter()
                        .take(SILENCE_LABELS)
                        .map(|(key, value)| LabelCond::Equals {
                            key: key.clone(),
                            value: value.clone(),
                        })
                        .collect(),
                    ..RouteMatch::default()
                },
                starts_at: now,
                ends_at: now + HOUR_MS,
                comment: Some("from a Slack button".into()),
            };
            if let Err(e) = state.store.create_silence(ws, &silence, &who, now).await {
                tracing::error!(error = %e, "could not create a silence from Slack");
                return StatusCode::INTERNAL_SERVER_ERROR;
            }
        }
    }
    StatusCode::OK
}
