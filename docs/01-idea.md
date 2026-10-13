# 01 — The idea

## Origin

Started from looking for a lightweight, open-source event bus for microservices
"like RabbitMQ/Kafka but super lightweight, mainly for notifications".
NATS (+ JetStream) covers the transport, but nothing covers the *notification*
part end to end: routing, channel connectors, delivery tracking and a UI.

## Problem

Internal / operational notifications ("order failed", "deploy finished",
"job X stuck", "customer upgraded plan") are usually hand-rolled:

- Slack/Teams webhook URLs copy-pasted into many services' configs
- Every service re-implements retries, formatting, rate limiting (or doesn't)
- No central view of what was sent, to whom, and whether it was delivered
- Changing "who gets notified about what" means a code change + redeploy
- Noisy: no dedup, grouping or quiet hours → alert fatigue

## Pitch

A single binary you drop next to your services:

1. **Ingest** — services POST an event; existing tools (Alertmanager, Grafana,
   GitHub, GitLab, Sentry) point their webhooks at it.
2. **Persist** — every event is stored; nothing is lost if Slack is down.
3. **Route** — rules match on topic / labels / payload and decide the targets.
4. **Shape** — dedup, grouping, rate-limit, quiet hours, escalation.
5. **Deliver** — connectors for Slack, Teams, email, Discord, webhook, ntfy,
   PagerDuty… with retries and per-delivery status.
6. **Observe & manage** — web UI to browse/search/replay messages and edit
   routes and connections live.

## Positioning (working hypothesis)

- *Not* Novu/Knock/Courier (product notifications to end users with inboxes,
  user preferences, templates for marketing-ish flows).
- *Is* infrastructure for **internal / ops notifications between services
  and teams** — closer to a generalised Alertmanager + Apprise with durable
  storage and a real UI. (Router only: the broker idea was dropped.)
- Differentiator: **one Rust binary, embedded storage, zero external deps**,
  trivial to self-host.

## Target users (hypothesis — to validate)

- Platform / DevOps teams in small–mid companies running microservices
- Agencies / consultancies standing up infra for clients (e.g. anvillabs)
- Indie SaaS builders who want ops alerts without wiring 5 webhooks
