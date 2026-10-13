//! What a route does with an event besides sending it: silence, dedup, grouping, quiet hours
//! and rate limits.
//!
//! Everything here is a pure function of its inputs. The engine applies the returned action;
//! the route tester only shows it.

use std::collections::BTreeMap;

use jiff::civil::{Time, Weekday};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};

use crate::event::{Event, EventStatus, Severity};
use crate::expr;
use crate::keys::hash_key;
use crate::matching::RouteMatch;
use crate::message::{Message, build_message};

const DAY_S: i64 = 86_400;
/// Titles listed in a digest before "and N more".
const DIGEST_LINES: usize = 10;

fn yes() -> bool {
    true
}

/// Events with the same key as an open notification are counted, not sent again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Dedup {
    /// An expression giving the key. Without it the event's own `dedup_key` is used, and
    /// failing that its topic and labels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// Seconds after a notification is created during which duplicates are absorbed.
    pub window_s: i64,
}

/// Events are collected and sent as one message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Group {
    /// Labels whose values define a group. Empty means one group per target.
    #[serde(default)]
    pub by: Vec<String>,
    /// Seconds to wait after the first event before sending.
    pub wait_s: i64,
    /// Seconds after which a sent notification that is still open is sent again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_s: Option<i64>,
}

/// A daily period in which notifications are held and sent as one digest when it ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct QuietHours {
    /// IANA time zone, for example `Europe/Warsaw`.
    pub tz: String,
    /// When the quiet period starts, `HH:MM`.
    pub start: String,
    /// When it ends, `HH:MM`. Earlier than `start` means the next morning.
    pub end: String,
    /// Days on which a quiet period starts: `mon` … `sun`. Empty means every day.
    #[serde(default)]
    pub days: Vec<String>,
    /// Events at or above this severity are sent anyway.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass_severity: Option<Severity>,
}

/// At most `max` notifications per window; the rest are held and sent as one summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct RateLimit {
    pub max: i64,
    pub window_s: i64,
}

/// A route's shaping policy. Empty means every matching event is sent at once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct Shaping {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedup: Option<Dedup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<Group>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quiet_hours: Option<QuietHours>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimit>,
    /// Tell a target when something it was notified about is resolved.
    #[serde(default = "yes")]
    pub notify_resolved: bool,
}

impl Default for Shaping {
    fn default() -> Self {
        Shaping {
            dedup: None,
            group: None,
            quiet_hours: None,
            rate_limit: None,
            notify_resolved: true,
        }
    }
}

fn parse_time(text: &str) -> Result<Time, String> {
    let (hour, minute) = text
        .split_once(':')
        .ok_or_else(|| format!("{text:?} is not a time; write it as HH:MM"))?;
    let (hour, minute) = (hour.parse::<i8>(), minute.parse::<i8>());
    match (hour, minute) {
        (Ok(hour), Ok(minute)) => {
            Time::new(hour, minute, 0, 0).map_err(|_| format!("{text:?} is not a time of day"))
        }
        _ => Err(format!("{text:?} is not a time; write it as HH:MM")),
    }
}

fn parse_day(text: &str) -> Result<Weekday, String> {
    Ok(match text.trim().to_ascii_lowercase().as_str() {
        "mon" => Weekday::Monday,
        "tue" => Weekday::Tuesday,
        "wed" => Weekday::Wednesday,
        "thu" => Weekday::Thursday,
        "fri" => Weekday::Friday,
        "sat" => Weekday::Saturday,
        "sun" => Weekday::Sunday,
        other => {
            return Err(format!(
                "{other:?} is not a day; use mon, tue, wed, thu, fri, sat or sun"
            ));
        }
    })
}

impl Shaping {
    pub fn is_empty(&self) -> bool {
        *self == Shaping::default()
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(dedup) = &self.dedup {
            if !(1..=30 * DAY_S).contains(&dedup.window_s) {
                return Err("dedup: the window must be 1 second to 30 days".into());
            }
            if let Some(key) = &dedup.key {
                expr::check_expression(key).map_err(|e| format!("dedup key: {e}"))?;
            }
        }
        if let Some(group) = &self.group {
            if !(1..=DAY_S).contains(&group.wait_s) {
                return Err("group: the wait must be 1 second to 1 day".into());
            }
            if group
                .repeat_s
                .is_some_and(|repeat| !(60..=30 * DAY_S).contains(&repeat))
            {
                return Err("group: the repeat interval must be 1 minute to 30 days".into());
            }
            if group.by.iter().any(|label| label.trim().is_empty()) {
                return Err("group: a label to group by must not be empty".into());
            }
        }
        if let Some(quiet) = &self.quiet_hours {
            TimeZone::get(&quiet.tz)
                .map_err(|_| format!("quiet hours: {:?} is not a known time zone", quiet.tz))?;
            parse_time(&quiet.start).map_err(|e| format!("quiet hours: {e}"))?;
            parse_time(&quiet.end).map_err(|e| format!("quiet hours: {e}"))?;
            for day in &quiet.days {
                parse_day(day).map_err(|e| format!("quiet hours: {e}"))?;
            }
        }
        if let Some(rate) = &self.rate_limit
            && (rate.max < 1 || !(1..=DAY_S).contains(&rate.window_s))
        {
            return Err(
                "rate limit: allow at least 1 notification per window of 1 second to 1 day".into(),
            );
        }
        Ok(())
    }
}

/// When the quiet period that `now_ms` falls in ends, or `None` outside quiet hours.
pub fn quiet_until(q: &QuietHours, now_ms: i64) -> Result<Option<i64>, String> {
    let tz = TimeZone::get(&q.tz).map_err(|_| format!("{:?} is not a known time zone", q.tz))?;
    let (start, end) = (parse_time(&q.start)?, parse_time(&q.end)?);
    let days: Vec<Weekday> = q
        .days
        .iter()
        .map(|d| parse_day(d))
        .collect::<Result<_, _>>()?;
    let on = |day: Weekday| days.is_empty() || days.contains(&day);
    let now = Timestamp::from_millisecond(now_ms)
        .map_err(|e| e.to_string())?
        .to_zoned(tz.clone())
        .datetime();
    let today = now.date();
    let yesterday = today.checked_sub(1.day()).map_err(|e| e.to_string())?;
    let tomorrow = today.checked_add(1.day()).map_err(|e| e.to_string())?;
    let overnight = end <= start;

    // A period belongs to the day it starts on.
    let ends = if overnight && on(yesterday.weekday()) && now.time() < end {
        // The one that began yesterday evening is still running.
        Some(today.to_datetime(end))
    } else if on(today.weekday()) && now.time() >= start && (overnight || now.time() < end) {
        Some(if overnight {
            tomorrow.to_datetime(end)
        } else {
            today.to_datetime(end)
        })
    } else {
        None
    };
    match ends {
        None => Ok(None),
        Some(civil) => {
            let at = civil.to_zoned(tz).map_err(|e| e.to_string())?;
            Ok(Some(at.timestamp().as_millisecond()))
        }
    }
}

/// A period during which matching events are not sent anywhere.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct Silence {
    pub id: String,
    pub r#match: RouteMatch,
    pub starts_at: i64,
    pub ends_at: i64,
    pub comment: Option<String>,
}

impl Silence {
    pub fn covers(&self, e: &Event, now_ms: i64) -> bool {
        self.starts_at <= now_ms && now_ms < self.ends_at && self.r#match.matches(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum NotificationState {
    /// Collecting events until its group's wait is over.
    Grouping,
    /// Waiting for quiet hours or a rate-limit window to end.
    Held,
    Sent,
    Acknowledged,
    Resolved,
    /// Not sent: silenced, or a resolution of something nobody was told about.
    Suppressed,
}

impl NotificationState {
    pub fn as_str(self) -> &'static str {
        match self {
            NotificationState::Grouping => "grouping",
            NotificationState::Held => "held",
            NotificationState::Sent => "sent",
            NotificationState::Acknowledged => "acknowledged",
            NotificationState::Resolved => "resolved",
            NotificationState::Suppressed => "suppressed",
        }
    }

    pub fn parse(s: &str) -> Option<NotificationState> {
        Some(match s {
            "grouping" => NotificationState::Grouping,
            "held" => NotificationState::Held,
            "sent" => NotificationState::Sent,
            "acknowledged" => NotificationState::Acknowledged,
            "resolved" => NotificationState::Resolved,
            "suppressed" => NotificationState::Suppressed,
            _ => return None,
        })
    }

    /// Still something that later events can join or resolve.
    pub fn is_open(self) -> bool {
        !matches!(
            self,
            NotificationState::Resolved | NotificationState::Suppressed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HoldKind {
    Quiet,
    Rate,
}

impl HoldKind {
    pub fn as_str(self) -> &'static str {
        match self {
            HoldKind::Quiet => "quiet",
            HoldKind::Rate => "rate",
        }
    }

    pub fn parse(s: &str) -> Option<HoldKind> {
        match s {
            "quiet" => Some(HoldKind::Quiet),
            "rate" => Some(HoldKind::Rate),
            _ => None,
        }
    }
}

/// How an event relates to the notification it is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// The event that opened the notification.
    First,
    /// Absorbed by dedup.
    Duplicate,
    /// Joined a group or a held digest.
    Grouped,
    /// The event that resolved it.
    Resolved,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::First => "first",
            Role::Duplicate => "duplicate",
            Role::Grouped => "grouped",
            Role::Resolved => "resolved",
        }
    }
}

/// An open notification of the target an event is being shaped for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenNotification {
    pub id: String,
    pub state: NotificationState,
    pub dedup_key: Option<String>,
    pub group_key: Option<String>,
    pub created_at: i64,
    /// A message has gone out for it.
    pub sent: bool,
    pub hold: Option<HoldKind>,
}

pub struct ShapeInput<'a> {
    pub event: &'a Event,
    pub shaping: &'a Shaping,
    /// Silences of the workspace; inactive ones are ignored.
    pub silences: &'a [Silence],
    /// Open notifications of this route target, oldest first.
    pub open: &'a [OpenNotification],
    /// Notifications sent to this target within the rate-limit window.
    pub sends_in_window: i64,
    pub oldest_send_in_window: Option<i64>,
    pub now_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Open a notification and send it now.
    Send {
        dedup_key: Option<String>,
        group_key: Option<String>,
    },
    /// Record the event without sending anything.
    Suppress {
        reason: String,
        silence_id: Option<String>,
    },
    /// Add the event to an existing notification without sending.
    Attach { notification_id: String, role: Role },
    /// Close an existing notification; `notify` when its target had been told about it.
    Resolve {
        notification_id: String,
        notify: bool,
    },
    /// Open a notification that collects events until `flush_at`.
    OpenGroup {
        group_key: String,
        dedup_key: Option<String>,
        flush_at: i64,
    },
    /// Keep the event until `until`: in the held notification `existing`, or a new one.
    Hold {
        existing: Option<String>,
        kind: HoldKind,
        until: i64,
        reason: String,
        dedup_key: Option<String>,
        group_key: Option<String>,
    },
}

/// One shaping step and what it concluded, for the journey and the route tester.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, utoipa::ToSchema)]
pub struct Step {
    /// `silence`, `resolve`, `dedup`, `group`, `quiet_hours`, `rate_limit` or `send`.
    pub step: &'static str,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub action: Action,
    pub steps: Vec<Step>,
}

/// The dedup key of an event under a policy. Without a dedup policy only the event's own key
/// counts; it lets a later resolved event find the notification.
pub fn dedup_key(shaping: &Shaping, e: &Event, steps: &mut Vec<Step>) -> Option<String> {
    let Some(dedup) = &shaping.dedup else {
        return e.dedup_key.clone();
    };
    if let Some(source) = &dedup.key {
        match expr::eval(source, &expr::event_context(e)) {
            Ok(serde_json::Value::String(key)) if !key.trim().is_empty() => {
                return Some(key.trim().chars().take(200).collect());
            }
            Ok(serde_json::Value::Null | serde_json::Value::String(_)) => {}
            Ok(other) => return Some(other.to_string().chars().take(200).collect()),
            Err(reason) => steps.push(Step {
                step: "dedup",
                outcome: format!("the key expression failed ({reason}); using the default key"),
            }),
        }
    }
    if let Some(own) = &e.dedup_key {
        return Some(own.clone());
    }
    let mut text = e.topic.clone();
    for (key, value) in &e.labels {
        text.push_str(&format!("|{key}={value}"));
    }
    Some(format!("auto:{}", &hash_key(&text)[..24]))
}

fn group_key(by: &[String], e: &Event) -> String {
    by.iter()
        .map(|label| {
            format!(
                "{label}={}",
                e.labels.get(label).map(String::as_str).unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn clock(ms: i64, tz: &str) -> String {
    let Ok(tz) = TimeZone::get(tz) else {
        return String::new();
    };
    Timestamp::from_millisecond(ms)
        .map(|t| t.to_zoned(tz).strftime("%H:%M").to_string())
        .unwrap_or_default()
}

/// Whether a notification of `severity` may go out now, or has to wait.
fn gate(
    shaping: &Shaping,
    severity: Severity,
    check_rate: bool,
    sends: i64,
    oldest: Option<i64>,
    now_ms: i64,
    steps: &mut Vec<Step>,
) -> Option<(HoldKind, i64, String)> {
    if let Some(quiet) = &shaping.quiet_hours {
        let bypass = quiet.bypass_severity.is_some_and(|min| severity >= min);
        match quiet_until(quiet, now_ms) {
            Ok(Some(until)) if !bypass => {
                let reason = format!(
                    "quiet hours until {} ({})",
                    clock(until, &quiet.tz),
                    quiet.tz
                );
                steps.push(Step {
                    step: "quiet_hours",
                    outcome: format!("held: {reason}"),
                });
                return Some((HoldKind::Quiet, until, reason));
            }
            Ok(Some(_)) => steps.push(Step {
                step: "quiet_hours",
                outcome: "quiet hours, but the severity is high enough to send anyway".into(),
            }),
            Ok(None) => steps.push(Step {
                step: "quiet_hours",
                outcome: "not in quiet hours".into(),
            }),
            Err(reason) => steps.push(Step {
                step: "quiet_hours",
                outcome: format!("ignored: {reason}"),
            }),
        }
    }
    if let (true, Some(rate)) = (check_rate, &shaping.rate_limit) {
        if sends >= rate.max {
            let until = oldest.unwrap_or(now_ms) + rate.window_s * 1000;
            let reason = format!(
                "rate limit: {} per {} s already sent",
                rate.max, rate.window_s
            );
            steps.push(Step {
                step: "rate_limit",
                outcome: format!("held: {reason}"),
            });
            return Some((HoldKind::Rate, until.max(now_ms + 1000), reason));
        }
        steps.push(Step {
            step: "rate_limit",
            outcome: format!("{sends} of {} sent in this window", rate.max),
        });
    }
    None
}

/// Decides what happens to an event for one target of a route that matched it.
pub fn shape(input: &ShapeInput<'_>) -> Decision {
    let ShapeInput {
        event,
        shaping,
        now_ms,
        ..
    } = *input;
    let mut steps = Vec::new();
    let done = |action: Action, steps: Vec<Step>| Decision { action, steps };

    if let Some(silence) = input.silences.iter().find(|s| s.covers(event, now_ms)) {
        let reason = match silence
            .comment
            .as_deref()
            .map(str::trim)
            .filter(|c| !c.is_empty())
        {
            Some(comment) => format!("silenced: {comment}"),
            None => "silenced".to_owned(),
        };
        steps.push(Step {
            step: "silence",
            outcome: reason.clone(),
        });
        return done(
            Action::Suppress {
                reason,
                silence_id: Some(silence.id.clone()),
            },
            steps,
        );
    }

    let key = dedup_key(shaping, event, &mut steps);
    let same_key = |n: &&OpenNotification| key.is_some() && n.dedup_key == key && n.state.is_open();

    if event.status == EventStatus::Resolved && key.is_some() {
        if let Some(open) = input.open.iter().rev().find(same_key) {
            let notify = open.sent && shaping.notify_resolved;
            steps.push(Step {
                step: "resolve",
                outcome: if notify {
                    "resolves an open notification; its target is told".into()
                } else {
                    "resolves an open notification that was not sent yet; nothing is sent".into()
                },
            });
            return done(
                Action::Resolve {
                    notification_id: open.id.clone(),
                    notify,
                },
                steps,
            );
        }
        if shaping.dedup.is_some() {
            let reason = "resolved, but nothing was open for it".to_owned();
            steps.push(Step {
                step: "resolve",
                outcome: reason.clone(),
            });
            return done(
                Action::Suppress {
                    reason,
                    silence_id: None,
                },
                steps,
            );
        }
    } else if let Some(dedup) = &shaping.dedup {
        let within = |n: &&OpenNotification| n.created_at >= now_ms - dedup.window_s * 1000;
        if let Some(open) = input.open.iter().rev().filter(same_key).find(within) {
            steps.push(Step {
                step: "dedup",
                outcome: "a notification with the same key is open; counted, not sent again".into(),
            });
            return done(
                Action::Attach {
                    notification_id: open.id.clone(),
                    role: Role::Duplicate,
                },
                steps,
            );
        }
        steps.push(Step {
            step: "dedup",
            outcome: "first event with this key in the window".into(),
        });
    }

    if let Some(group) = &shaping.group {
        let gk = group_key(&group.by, event);
        let collecting = input.open.iter().rev().find(|n| {
            n.state == NotificationState::Grouping && n.group_key.as_deref() == Some(gk.as_str())
        });
        if let Some(open) = collecting {
            steps.push(Step {
                step: "group",
                outcome: "joins a group that is still collecting".into(),
            });
            return done(
                Action::Attach {
                    notification_id: open.id.clone(),
                    role: Role::Grouped,
                },
                steps,
            );
        }
        steps.push(Step {
            step: "group",
            outcome: format!("opens a group; it is sent after {} s", group.wait_s),
        });
        return done(
            Action::OpenGroup {
                group_key: gk,
                dedup_key: key,
                flush_at: now_ms + group.wait_s * 1000,
            },
            steps,
        );
    }

    let held = gate(
        shaping,
        event.severity,
        true,
        input.sends_in_window,
        input.oldest_send_in_window,
        now_ms,
        &mut steps,
    );
    if let Some((kind, until, reason)) = held {
        let existing = input
            .open
            .iter()
            .rev()
            .find(|n| n.state == NotificationState::Held && n.hold == Some(kind))
            .map(|n| n.id.clone());
        return done(
            Action::Hold {
                existing,
                kind,
                until,
                reason,
                dedup_key: key,
                group_key: None,
            },
            steps,
        );
    }
    steps.push(Step {
        step: "send",
        outcome: "sent".into(),
    });
    done(
        Action::Send {
            dedup_key: key,
            group_key: None,
        },
        steps,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseFrom {
    /// A group whose wait is over.
    Group,
    /// A held notification whose hold is over.
    Hold,
}

pub struct ReleaseInput<'a> {
    pub shaping: &'a Shaping,
    /// The highest severity among the notification's events.
    pub max_severity: Severity,
    pub from: ReleaseFrom,
    pub sends_in_window: i64,
    pub oldest_send_in_window: Option<i64>,
    pub now_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Released {
    Send,
    Hold {
        kind: HoldKind,
        until: i64,
        reason: String,
    },
}

/// Decides whether a notification that is ready may go out now. A group is checked against
/// quiet hours and the rate limit; a hold that ended is the summary and only waits for quiet hours.
pub fn release(input: &ReleaseInput<'_>) -> Released {
    let mut steps = Vec::new();
    match gate(
        input.shaping,
        input.max_severity,
        input.from == ReleaseFrom::Group,
        input.sends_in_window,
        input.oldest_send_in_window,
        input.now_ms,
        &mut steps,
    ) {
        Some((kind, until, reason)) => Released::Hold {
            kind,
            until,
            reason,
        },
        None => Released::Send,
    }
}

fn severity_word(severity: Severity) -> &'static str {
    match severity {
        Severity::Debug => "debug",
        Severity::Info => "info",
        Severity::Warning => "warning",
        Severity::Error => "error",
        Severity::Critical => "critical",
    }
}

/// The message for a notification's events: the event itself when there is one, a summary
/// when there are several.
pub fn digest(events: &[Event], public_url: Option<&str>, notification_id: &str) -> Message {
    let Some(first) = events.first() else {
        return Message {
            title: "No events".into(),
            body: None,
            severity: Severity::Info,
            status: EventStatus::Firing,
            topic: String::new(),
            fields: Vec::new(),
            ..Message::default()
        };
    };
    if events.len() == 1 {
        return build_message(first, public_url);
    }
    let count = events.len();
    let same_title = events.iter().all(|e| e.title == first.title);
    let same_topic = events.iter().all(|e| e.topic == first.topic);
    let title = if same_title {
        format!("{} ({count} times)", first.title)
    } else if same_topic && count > DIGEST_LINES {
        format!("{count} events on {}", first.topic)
    } else {
        format!("{count} events")
    };
    let body = if same_title {
        first.body.clone()
    } else {
        let mut lines: Vec<String> = events
            .iter()
            .take(DIGEST_LINES)
            .map(|e| format!("{}: {}", severity_word(e.severity), e.title))
            .collect();
        if count > DIGEST_LINES {
            lines.push(format!("and {} more", count - DIGEST_LINES));
        }
        Some(lines.join("\n"))
    };
    let common: BTreeMap<&String, &String> = first
        .labels
        .iter()
        .filter(|(key, value)| events.iter().all(|e| e.labels.get(*key) == Some(value)))
        .collect();
    Message {
        title,
        body,
        severity: events
            .iter()
            .map(|e| e.severity)
            .max()
            .unwrap_or(first.severity),
        status: if events.iter().any(|e| e.status == EventStatus::Firing) {
            EventStatus::Firing
        } else {
            EventStatus::Resolved
        },
        topic: if same_topic {
            first.topic.clone()
        } else {
            "several topics".into()
        },
        fields: common
            .into_iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        link: public_url.map(|base| {
            format!(
                "{}/notifications/{notification_id}",
                base.trim_end_matches('/')
            )
        }),
        event_ids: events.iter().map(|e| e.id.clone()).collect(),
        ..Message::default()
    }
}
