# 04 — Open questions

> Answer inline under each question (replace the `_Answer:_` line).

## Product & market

### Q1. Who is the user/buyer?
Platform/DevOps teams? Product engineers? Agencies? Indie SaaS builders?
Internal ops notifications only, or also notifications to end users?

_Answer (2026-10-13, working assumption, not validated):_ Platform / DevOps
teams and small SaaS builders. Internal and operational notifications only.

### Q2. What's the business model?
Pure OSS hobby project? Open-core (OSS self-hosted + paid features)?
Hosted SaaS? Something you'd use inside anvillabs / client projects first?

_Answer (2026-10-13):_ Public OSS launch within the first 3–6 months, built to
be adopted by others. Open source, with some parts monetized (open-core);
same licensing model as Tayga:

- Core: `AGPL-3.0-only`.
- Enterprise edition under a commercial licence, which also removes the AGPL
  obligations. Contributors sign a CLA so the code can be offered under both.
- Client libraries / SDKs: permissive (Apache-2.0 or MIT), so they can be
  linked into proprietary services.
- Proposed split (not yet confirmed): free = ingest, routing, all shaping
  features, all connectors, full UI, SQLite single node, local users.
  Paid = SSO, RBAC / per-team scoping, audit log, long retention/export,
  HA (Postgres, multiple nodes), hosted version, support.
- v1 builds no paid features, only the seams: workspace id on every table,
  pluggable auth provider, storage behind a trait.

### Q3. Is the broker part actually needed?
Do users need NATS-style pub/sub (services consuming each other's messages),
or just "POST an event, get it routed reliably to channels"?

_Answer (2026-10-13):_ Router only. Services POST events; ramzes persists,
routes and delivers. No pub/sub, no NATS wire protocol. Outbound webhooks
cover the "another service reacts" case.

### Q4. Have you (or people you know) felt this pain?
Concrete stories: where are notifications hand-rolled today? (VirtusLab,
SoftwareMill, client projects, your own apps like Langusta / rustcoach)

_Answer:_

## Scope

### Q5. Which channels matter for v1?
Slack, MS Teams, email, webhook, Discord, ntfy, PagerDuty, SMS, Telegram…?

_Answer (2026-10-13):_ All four groups get first-class support at launch:
team chat (Slack, MS Teams, Discord), email (SMTP), paging (PagerDuty,
Opsgenie), personal push (ntfy, Telegram, Pushover). Plus a generic webhook
and declarative HTTP connectors for the long tail. No WASM plugins in v1.

### Q6. Which shaping features are must-have vs. later?
Dedup, grouping/digests, rate limiting, quiet hours, escalation, on-call
schedules, acknowledgements?

_Answer (2026-10-13):_ All wanted for the public launch: dedup + rate limit,
grouping / digests, quiet hours + silences, escalation + acks. Retries,
delivery status and replay are baseline. On-call schedules not requested.
Build order across milestones still to be decided.

### Q6b. Which inbound sources work without writing code?

_Answer (2026-10-13):_ Monitoring webhooks (Alertmanager, Grafana), dev-tool
webhooks (GitHub, GitLab, Sentry) and a catch-all "any JSON + mapping"
endpoint with per-source mappings defined in the UI. No inbound email.

### Q6c. Where does configuration live?

_Answer (2026-10-13):_ In the database, edited live from the UI. The database
is SQLite by default or Postgres, chosen in the instance config.

### Q7. Single-tenant self-hosted only, or multi-tenant from day one?

_Answer (2026-10-13):_ Single tenant per instance. Every table carries a
`workspace_id` so team scoping or a hosted version can be added later.

## Relationship to other projects

### Q8. Does this connect to the real-time log correlation tool?
"Detect (log correlation) → notify (ramzes)" could be one story / one
open-core product family. Separate or combined?

_Answer (2026-10-13):_ Fully separate from Tayga. No planned link; any
integration goes through the generic HTTP ingest like any other service.

### Q9. How much time can you put into it, and what's the first milestone?
Weekend project, side project with a launch date, or something bigger?

_Answer (2026-10-13):_ Full-time. Public launch targets the full feature set,
built in four milestones, each with its own spec and plan:
1. Core — ingest, storage, routing, retries, delivery status, replay,
   Slack + webhook, UI shell with message browser.
2. Breadth — remaining connectors, declarative connector format, route and
   connection editors.
3. Noise control — dedup, rate limit, grouping/digests, quiet hours, silences.
4. Urgency — escalation, acks, paging connectors, interactive chat actions.
