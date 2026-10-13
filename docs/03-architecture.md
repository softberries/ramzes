# 03 — Architecture

> The first sketch that lived here has been replaced by the agreed design:
> [superpowers/specs/2026-10-13-ramzes-design.md](superpowers/specs/2026-10-13-ramzes-design.md).
> That file is the source of truth. This page is a summary.

```
 services ───────── POST /v1/events ──────────┐
 Alertmanager, Grafana, GitHub, GitLab, Sentry ┤  per-source URL + mapping
 any JSON sender ──────────────────────────────┤
                                               ▼
                                   ┌──────────────────────┐
                                   │ Ingest               │  verify, map to events,
                                   │                      │  write, then reply 202
                                   └──────────┬───────────┘
                                              ▼
                                   ┌──────────────────────┐
                                   │ Database             │  SQLite (default) or Postgres
                                   │ events · deliveries  │  the database is the queue
                                   │ timers · config      │
                                   └──────────┬───────────┘
                     ┌────────────────────────┼────────────────────────┐
                     ▼                        ▼                        ▼
              Route worker             Delivery worker            Timer worker
              match, silence,          render, send, retry,       group flush, quiet-hours
              dedup, group, quiet      dead-letter                end, escalation step
              hours, rate limit
                                              │
        ┌──────────┬──────────┬───────────┬───┴──────┬───────────┬────────────┐
        ▼          ▼          ▼           ▼          ▼           ▼            ▼
      Slack     MS Teams   Discord      Email    PagerDuty /   ntfy /      webhook +
                                        (SMTP)   Opsgenie      Telegram /  declarative
                                                               Pushover    connectors

                  Web UI (SvelteKit, embedded) ⇄ Admin API (Axum)
        live feed · event journey · routes + tester · connections · sources
```

## Decisions

- **Router only.** No pub/sub, no NATS protocol, no OTel ingest.
- **The database is the queue.** Every stage is rows; workers claim due rows
  with a lease. Nothing lives only in memory.
- **SQLite by default, Postgres optional**, chosen in the instance config.
- **Config lives in the database**, edited live in the UI, versioned, and
  exportable as YAML.
- **Connectors are declarative first.** A YAML definition covers any service
  that is one HTTP request; Rust code only where that is not enough (SMTP,
  Slack app mode). No WASM plugins.
- **Delivery is at-least-once.**
- **Licence:** core `AGPL-3.0-only`, enterprise edition under a commercial
  licence, CLA for contributors.

## Milestones

1. **Core** — ingest, storage, routing, retries, replay, Slack + webhook,
   first-run setup, live feed, event journey.
2. **Breadth** — declarative connectors and adapters, remaining channels and
   sources, templates, route tester, config history and export/import.
3. **Noise control** — silences, dedup, grouping, quiet hours, rate limit.
4. **Urgency** — escalation, acknowledgements, paging, Slack buttons.

All four milestones are built. The public launch still needs the checks in
[05-next-steps.md](05-next-steps.md).
