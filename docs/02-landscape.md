# 02 — Landscape

> Written from general knowledge on 2026-10-13, not yet verified with a fresh
> search. Re-check positioning (especially Novu's self-hosting story) before
> committing.

## Closest competitors

| Tool | What it is | Why it's not quite this |
|---|---|---|
| **Novu** | OSS notification infrastructure: workflows, dashboard, email/SMS/push/chat/in-app | Aimed at *product* notifications to end users. Heavy self-host (Node, MongoDB, Redis, several services) |
| **Knock / Courier / SuprSend** | Commercial SaaS notification platforms | SaaS, product-notification focus, per-message pricing |
| **Apprise + apprise-api** | Python lib/server, sends to 100+ services | No broker, no persistence, minimal routing, basic UI |
| **Prometheus Alertmanager** | Routing, grouping, dedup, silences → Slack/email/PagerDuty | Only for Prometheus alerts; config-file driven, no message browser |
| **Svix / Hookdeck** | Reliable webhook sending/receiving with UI | Webhook delivery only, not multi-channel notification routing |
| **ntfy / Gotify** | Self-hosted push to phones/desktops via HTTP | Single-channel endpoint, not a router |
| **NATS + JetStream** | Lightweight broker with persistence | Transport only — no connectors, routing rules or UI |
| **n8n / Zapier / Make** | General workflow automation | Too general and heavy for "route this event to #ops"; not built for volume |

## The gap

Nobody combines all of:

1. Broker-grade ingestion with persistence
2. Notification-specific routing (dedup, grouping, quiet hours, escalation)
3. Built-in multi-channel connectors
4. A UI to browse messages, see delivery state, replay, and edit routes live
5. Trivial self-hosting (single binary, no external DB required)

for **internal / ops** use.

## Risks

- Novu (or Alertmanager-adjacent tools like Grafana OnCall / Keep) could cover
  the internal use case well enough.
- "Notifications" might be a feature people build into existing tools rather
  than a product they adopt.
- Connectors are a long tail to maintain (APIs change, auth flows differ).
