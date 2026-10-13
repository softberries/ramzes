# Milestone 3 (Noise control) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Tests first.

**Goal:** Routes shape what they send: silences, dedup, grouping, quiet hours and rate limits; resolved events close notifications; failing connections are paused; ramzes reports its own delivery failures as events.

**Architecture:** Every shaping decision is a pure function in `ramzes-core::shaping` of the event, the route's policy, the open notifications of the target and the time. It returns one action plus the steps that led to it. The route worker applies actions in one transaction; a timer worker flushes groups, releases held notifications and sends reminders. Shaping and timers run under one in-process lock, because ramzes is single-node.

**Tech Stack:** as milestone 2, plus `jiff` (time zones, bundled tz database).

**Spec:** `docs/superpowers/specs/2026-10-13-ramzes-design.md` §4 (states), §5 (shaping, resolution), §7 (screens marked 3), §8 (system topic), §9 (connection pausing).

## Global Constraints

- All constraints of the milestone 1 and 2 plans hold. Schema changes go in `0003_noise.sql`.
- A route without a shaping policy behaves exactly as before: one notification and one delivery per matched target.
- Replays bypass shaping: a replayed event or delivery is sent.
- A shaping decision never loses an event: every event ends attached to a notification whose state and reason explain what happened.
- Times in policies are whole seconds; quiet hours are wall-clock times in a named IANA time zone.

## Review Focus

1. A group's timer firing while an event joins the group: the event is in the message or in the next one, never in neither.
2. A resolved event arriving before, during and after grouping or holding: no message is sent for something nobody was told about.
3. Quiet hours across midnight and across a daylight-saving change: release happens at the configured wall-clock time.
4. A timer whose notification was deleted by retention, or acknowledged meanwhile: nothing is sent, the timer is removed.
5. A system event about a failed delivery of a system event: no loop.

---

### Task 1: Shaping in core

**Files:** `crates/ramzes-core/src/shaping.rs`, `routing.rs` (Route gains `shaping`), tests `tests/shaping.rs`.

**Produces:**
- `Shaping { dedup: Option<Dedup{key, window_s}>, group: Option<Group{by, wait_s, repeat_s}>, quiet_hours: Option<QuietHours{tz, start, end, days, bypass_severity}>, rate_limit: Option<RateLimit{max, window_s}>, notify_resolved: bool }` with `validate()`
- `Silence { id, match: RouteMatch, starts_at, ends_at, comment }`, `Silence::covers(&Event, now)`
- `OpenNotification { id, state, dedup_key, group_key, created_at, held_for }`, `NotificationState`
- `shape(&ShapeInput) -> Decision { action: Action, steps: Vec<Step> }`
- `Action::{Send, Suppress, Attach, Resolve, OpenGroup, Hold}`
- `release(&ReleaseInput) -> Released::{Send, Hold{until, reason}}` for a group that flushes or a hold that ends
- `quiet_until(&QuietHours, now_ms) -> Result<Option<i64>, String>`
- `digest(&[Event], public_url, notification_id) -> Message`

**Tests:** table-driven, fake clock: no policy sends; silence suppresses and names the silence; dedup attaches within the window and sends after it; resolved closes an open notification and notifies only if it had been sent; resolved with nothing open is suppressed when dedup is on and sent when it is off; grouping opens then joins; quiet hours hold until the end, across midnight, bypass by severity, days; DST day; rate limit holds after the cap until the window ends; held notifications of a target merge; digest wording for one, same-title and mixed events.

### Task 2: Schema 0003 and store

**Schema:** `notifications` gains `target_key, sent_at, sent_count, held_until, silence_id, resolved_at, first_title`; `notification_events.role`; `timers.data`; `connections` gains `paused_until, failure_streak`; indexes on `(workspace_id, target_key, state)` and `timers(due_at)`.

**Produces:** routes read/write `shaping_json`; silences CRUD and `active_silences`; `open_notifications`, `sends_since`; `write_routing` accepting new notifications in any state, joins and resolves; timers `claim_due_timers`, `delete_timer`, `cancel_timers`, `add_timer`; `send_notification`, `hold_notification`, `merge_notifications`; `acknowledge`, `resolve`; `list_notifications`, `get_notification`; connection `record_outcome`, `pause`, `resume`, `defer_deliveries`; `system_source`; `overview`.

**Tests (both databases):** each function's round trip and its conditional cases (joining a notification that is no longer grouping is refused; cancelling timers; pausing defers pending deliveries).

### Task 3: Engine

Route worker applies shaping under the lock; timer worker (`timers_once`) flushes, releases and reminds; delivery worker builds digests, records connection outcomes, pauses and resumes; system events for dead and rejected deliveries and for paused connections, never about `ramzes.*` topics.

**Tests:** end-to-end through the engine with a fake clock for every Review Focus item.

### Task 4: Admin API

Route `shaping`; `/api/silences` CRUD; `/api/notifications` list, detail, `acknowledge`, `resolve`; `/api/connections/{id}/resume` and paused state; `/api/overview`; the route tester returns shaping steps; journey shows each event's role.

### Task 5: UI

Shaping section in the route editor; Notifications, Silences and Overview screens; paused connections; tester shows shaping.

### Task 6: Docs and review
