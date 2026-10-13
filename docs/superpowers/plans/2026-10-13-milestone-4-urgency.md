# Milestone 4 (Urgency) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Tests first.

**Goal:** Notifications that nobody acknowledges escalate; people can acknowledge from the UI, the API, a signed link in the message and Slack buttons; PagerDuty and Opsgenie are supported.

**Architecture:** A route carries an escalation chain of steps (a delay and targets). When a notification is first sent, a timer is armed for the first step; each step that fires while the notification is neither acknowledged nor resolved sends to its targets and arms the next. Every message carries a link whose token is an HMAC of the notification id under the instance key; the link opens a confirmation page and acknowledges only on submit. Slack app mode is a hand-built connector that posts with a bot token, adds buttons, threads follow-ups and rewrites its message when the notification is acknowledged or resolved.

**Spec:** `docs/superpowers/specs/2026-10-13-ramzes-design.md` §5 (resolution, acknowledgement, escalation), §6 (hand-built connectors), §7 (escalation view).

## Global Constraints

- All constraints of the earlier plans hold. Schema changes go in `0004_urgency.sql`.
- An acknowledgement link never acts on a GET: scanners and link previews issue GET requests.
- A token that does not match, or a Slack request whose signature or timestamp is wrong, changes nothing and reveals nothing about the notification.
- An escalation step never fires for a notification that is acknowledged, resolved or was never sent.

## Review Focus

1. Acknowledging at the same moment a step fires: at most that one step goes out, none after.
2. A route edited or deleted while a chain is running: remaining steps follow the route as it is now; a deleted route stops the chain.
3. A resolved notification: everyone who was told, including escalation targets, is told it is resolved; nobody else is.
4. A replayed or forged Slack interaction (old timestamp, wrong secret, another connection's secret): rejected.
5. An acknowledgement link opened twice, or for a notification that is gone: a clear page, no error.

---

### Task 1: Core — escalation chain, message fields

`Route.escalation: Vec<EscalationStep { after_s, targets }>` with validation (1 s to 7 days, at most 10 steps, each with a target). `Message` gains `dedup_key`, `ack_link`, `notification_id`, `thread_ref`.

### Task 2: Schema 0004 and store

`deliveries.template_id`, `deliveries.step`; routes read and write `escalation_json`; `add_deliveries` for a notification; `escalated_targets`; `first_external_ref`; resolving tells a given list of targets.

### Task 3: Engine

Arms and fires escalation timers; fills the message's dedup key, acknowledgement link and thread reference; `refresh_messages` asks connectors that can update a message to do so after an acknowledgement or resolution.

### Task 4: Connectors

PagerDuty (trigger and resolve by dedup key) and Opsgenie (create and close by alias) as bundled definitions; Slack app mode (post, thread, buttons, update); the acknowledgement link in the other connectors' default formats.

### Task 5: API

`GET` and `POST /ack/{id}`; `POST /v1/slack/{connection_id}/interactions`; route `escalation`; escalation progress on the notification detail; the tester shows the chain.

### Task 6: UI

Escalation chain editor on routes; chain progress on the notification detail; acknowledgement page styling.

### Task 7: Docs, final review of the whole work
