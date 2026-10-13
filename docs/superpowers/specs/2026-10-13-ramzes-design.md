# ramzes — design

Date: 2026-10-13
Status: awaiting review

This is the overall design for ramzes, covering all four milestones up to the
public launch. Each milestone gets its own implementation plan; the first plan
written from this spec covers milestone 1 only.

## 1. Purpose

ramzes is a self-hosted notification router for internal and operational
events. Services and tools send it events over HTTP. It stores every event,
decides who should hear about it, cuts the noise, delivers to chat, email,
paging and push channels, and shows in a web UI what happened to each message
and why.

It is for platform and DevOps teams and small SaaS builders. It is not for
product notifications to end users (the Novu / Knock space).

### Goals, in priority order

1. **Simplest deployment.** One binary, no required external services. Running
   it with no arguments gives a working instance.
2. **A UI people want to show others.** Every event's path is visible and
   explained; config is edited live.
3. **Many integrations**, inbound and outbound, added mostly as data files
   rather than code.

### Success criteria

- From download to the first message in a real channel in under five minutes.
- No acknowledged event is lost across a crash or restart.
- For any event, the UI answers "was it delivered, to whom, and if not, why".
- A new plain-HTTP integration can be added without writing Rust.

### Out of scope

- Pub/sub between services, the NATS wire protocol, OTel ingest.
- A WASM or other plugin runtime.
- Inbound email.
- On-call schedules and rotas.
- Client libraries (an OpenAPI description and curl examples ship instead).
- Any link to Tayga beyond the generic HTTP ingest.
- Paid features. Only their seams are built (section 10).

## 2. Decisions taken

| Topic | Decision |
|---|---|
| Launch | Public open-source launch with the full feature set; full-time work |
| Core | Router only: ingest, persist, route, shape, deliver |
| Pipeline | The database is the queue. Every stage is rows; nothing lives only in memory |
| Database | SQLite by default or Postgres, chosen in the instance config |
| Configuration | Stored in the database, edited live in the UI, versioned, exportable as YAML |
| Outbound channels | Slack, MS Teams, Discord, email (SMTP), PagerDuty, Opsgenie, ntfy, Telegram, Pushover, generic webhook, plus declarative HTTP connectors |
| Inbound sources | Native event API, Alertmanager, Grafana, GitHub, GitLab, Sentry, and any JSON with a user-defined mapping |
| Shaping | Silences, dedup, grouping/digests, quiet hours, rate limit, escalation, acknowledgements |
| Tenancy | Single tenant per instance; `workspace_id` on every table as a seam |
| Licence | Core `AGPL-3.0-only`; enterprise edition under a commercial licence; CLA for contributors; any future client libraries Apache-2.0 or MIT |
| Visual direction | "Control room": dark, dense, monospaced. Dark by default with a light theme |

## 3. Architecture

### Deployment

- One binary, `ramzes`. With no arguments it creates a SQLite database in a
  data directory and prints a first-run setup URL.
- Instance settings come from environment variables or one TOML file: listen
  address, database URL, data directory, secret key, outbound address policy.
  Everything else is configured in the UI.
- The web UI is built to static files and embedded in the binary.
- TLS is terminated by a reverse proxy.

### Workspace layout

| Crate | Responsibility | Depends on |
|---|---|---|
| `ramzes-core` | Domain types and pure logic: events, routes, matching, shaping decisions, the canonical message. No I/O | — |
| `ramzes-store` | One storage interface with SQLite and Postgres implementations; migrations | core |
| `ramzes-ingest` | HTTP ingest endpoints and source adapters | core, store |
| `ramzes-engine` | Route, delivery and timer workers | core, store, connectors |
| `ramzes-connectors` | Outbound connector interface, hand-built connectors, the declarative HTTP connector | core |
| `ramzes-api` | Admin API, auth, live update stream | core, store |
| `ramzes-server` | The binary: wiring, CLI, embedded UI assets | all |
| `ui/` | SvelteKit app | the API's OpenAPI description |

### Data flow

1. **Ingest.** Authenticate the source. Its adapter maps the request to one or
   more events. The events are written in one transaction with `routed_at`
   null. Only then does ramzes reply 202.
2. **Route worker.** Claims unrouted events and evaluates the routes. For each
   matched target it runs the shaping steps (section 5) and writes either a
   delivery due now or a timer due later, then sets `routed_at`.
3. **Delivery worker.** Claims due deliveries, renders the template, calls the
   connector and records the attempt. Retryable failures are rescheduled;
   exhausted ones are dead-lettered.
4. **Timer worker.** Claims due timers: flush a group, end quiet hours, close a
   rate-limit window, run an escalation step.

Workers are woken by an in-process signal when work is written and also poll
on an interval. The UI receives changes over a server-sent event stream.

Delivery is **at-least-once**. A crash between sending and recording the
result can produce a duplicate message.

## 4. Data model

| Table | Holds |
|---|---|
| `workspaces` | One row in v1 |
| `users`, `sessions`, `api_tokens` | Local accounts, browser sessions, admin API tokens |
| `sources` | Name, adapter kind, hashed ingest key, mapping definition |
| `source_requests` | The most recent raw requests per source, with any mapping error |
| `events` | Topic, severity, status (firing / resolved), title, body, payload JSON, optional dedup key, idempotency key, occurred and received times, `routed_at` |
| `event_labels` | `(event_id, key, value)`, indexed |
| `connections` | Connector kind, name, settings, encrypted credentials |
| `connector_definitions` | User-added declarative connector definitions |
| `templates` | Template per connector kind |
| `routes` | Position, match, targets, shaping policy, escalation chain, enabled flag |
| `silences` | Matcher, start, end, author, comment |
| `notifications` | One per route target and group key: state, reason, dedup key, counts, acknowledged-by and time |
| `notification_events` | Events belonging to a notification |
| `deliveries` | Notification, connection, destination, state, attempt count, `next_attempt_at`, lease, last error, external reference |
| `delivery_attempts` | Time, duration, response status, response excerpt |
| `timers` | Kind, `due_at`, lease, notification |
| `config_revisions` | Full config snapshot per change, author, time |

Every table except `workspaces` carries `workspace_id`.

**Notification states:** `grouping`, `held` (quiet hours or rate limit),
`sent`, `acknowledged`, `resolved`, `suppressed` (silenced). The `reason`
column records why the notification is in its state.

**Delivery states:** `pending`, `in_progress`, `delivered`, `failed`
(rejected, not retried), `dead` (retries exhausted).

### Rules that keep both databases identical in behaviour

- Ids are time-ordered UUIDs generated in the application.
- Work is claimed with a lease: a worker sets `claimed_until` on due rows
  whose lease has expired. Rows held by a crashed worker become claimable when
  the lease runs out.
- Dedup and rate-limit checks are indexed lookups within a time window.
- SQL is hand-written and limited to what both databases support, behind the
  one storage interface.
- Labels are rows, not JSON queries.

### Search and retention

- Search in milestone 1 is filtering by time, source, topic, severity, label
  and outcome, plus substring match on the title. Full-text search is a later
  addition behind the storage interface.
- Retention is one instance setting with per-topic overrides. A background job
  deletes expired events and their dependent rows in batches.

## 5. Routing and shaping

### Routes

- An ordered list. An event is checked against each route from the top. Every
  match fires unless a matching route is marked "stop here".
- An event that matches no route is kept and shown as unrouted.
- A route has a match, one or more targets (connection, destination, optional
  template) and a shaping policy.

### Matching

- Structured conditions, all of which must hold: topic pattern, severity
  threshold, source, label equals / matches / exists.
- An optional expression over the payload, in the template engine's expression
  syntax, so matching, templates and inbound mappings share one language.

### Shaping steps, in order, per matched target

| Step | Behaviour | Settings |
|---|---|---|
| Silence | An active matching silence suppresses the notification and records the silence | Matcher, start, end |
| Dedup | An event whose key matches an open notification is attached and counted, not sent again | Key (default: topic + labels), window |
| Grouping | The first event opens a group and starts a timer; later events join; one message is sent on flush | Group-by labels, initial wait, repeat interval |
| Quiet hours | Outside allowed hours the notification is held and released as one digest when they end; severities at or above a threshold bypass | Time zone, days, hours, bypass severity |
| Rate limit | Over the cap, notifications are held and sent as one summary when the window closes | Max per window per target |

### Resolution, acknowledgement and escalation

- A resolved event with the same dedup key closes the notification and
  cancels its pending timers.
- A route can carry an escalation chain of steps: a delay and targets. A step
  runs if the notification is neither acknowledged nor resolved when its timer
  fires.
- Acknowledgement is possible from the UI, the API and a signed link included
  in every message. The link opens a confirmation page and acknowledges only
  on an explicit submit, because scanners and link previews issue GET
  requests.
- Slack buttons for acknowledge and silence require a Slack app. Slack
  connections therefore have two modes: incoming webhook (send only) and app
  (buttons, message updates, threads).

### Replay and dry run

- Replay resends one delivery, or re-runs an event through the current routes.
  Replayed notifications are marked as such.
- The route tester evaluates a past or sample event against the current,
  possibly unsaved, routes and returns the matches and the shaping outcome
  without writing anything.

Every shaping decision is a pure function in `ramzes-core` of the event, the
route, the relevant existing rows and the current time. It returns a list of
actions (create delivery, set timer, attach to notification, suppress). The
engine applies them; the tester only displays them.

## 6. Connectors

### Outbound

- **Canonical message.** The engine produces one neutral message: title, body,
  severity, fields, links, actions, and the event list for digests. Each
  connector ships a default template that renders it to the connector's own
  format. A route can override the template.
- **Connector interface.** Each connector provides its settings schema (which
  drives the setup form), its capabilities (rich formatting, message update,
  threads, buttons, length limit), a connection test, and `send`, which
  returns delivered with an external reference, retry (with an optional
  delay), or permanent failure.
- **Declarative connectors.** A YAML definition with settings fields, request
  method, URL, headers and body as templates, success status codes, and the
  name of the service's rate-limit header. Bundled definitions are compiled
  into the binary. Users add their own in the UI. The community adds bundled
  ones by pull request.
- **Hand-built connectors** are written only when the declarative format
  cannot express the service: a non-HTTP protocol, OAuth, multi-step calls or
  interactivity. SMTP and Slack app mode are hand-built. The rest of the
  launch list is expected to be declarative; each is confirmed against the
  service's current API documentation when its milestone is planned.
- **Shared delivery behaviour:** per-connection concurrency limit, honouring
  the service's retry-after signal, backoff with jitter, a "send test message"
  action.
- **Generic webhook** signs each request with a per-connection secret.
- **Outbound address policy.** An instance setting blocks requests to private
  network addresses. It is off by default.

### Inbound

- **Native endpoint.** `POST /v1/events` with a source key, one event or a
  batch. An idempotency key makes client retries safe.
- **Per-source URLs** for third-party senders, each with its own secret.
- **Source adapters** verify the request (bearer token, signature header, or
  none) and map the payload to one or more events.
- **Adapters are declarative.** A mapping definition holds the verification
  method and field mappings written as expressions. Alertmanager, Grafana,
  GitHub, GitLab and Sentry ship as bundled definitions where their formats
  allow, and as code where they do not.
- **Any-JSON sources** use the same mechanism with a mapping written in the
  UI, previewed live against the source's recent raw requests.
- The original payload is always kept on the event.

## 7. Web UI

### Stack

- SvelteKit built as a static single-page app in TypeScript.
- API types generated from the server's OpenAPI description.
- Live updates over the server-sent event stream.

### Visual direction

"Control room": dark background, dense rows, monospaced type, colour reserved
for severity and delivery state. Dark is the default; a light theme is
available and follows the same tokens. All colours are defined as theme tokens
from the first commit, so neither theme is an afterthought.

### Screens

| Screen | Purpose | Milestone |
|---|---|---|
| First-run setup | Create admin, create a source, send a test event with a copyable curl command, add a connection, create the first route | 1 |
| Live feed | Events streaming in; filter bar; pause; keyboard navigation; filters held in the URL | 1 |
| Event journey | Timeline of one event: received, routes matched, each shaping decision with its reason, each delivery with attempts and responses; raw payload; rendered message preview; replay | 1 |
| Failures | Dead-lettered and failed deliveries grouped by error; bulk replay | 1 |
| Routes | Reorderable list and form editor (1); tester side panel (2) | 1, 2 |
| Connections | Setup form and test button (1); connector catalogue and health (2) | 1, 2 |
| Sources | Keys, URLs, curl snippet (1); recent requests and mapping editor (2) | 1, 2 |
| Settings | Users, tokens, retention (1); config history with diff and rollback, export/import (2) | 1, 2 |
| Templates | Editor with live preview per channel | 2 |
| Notifications | Open, held, grouping and escalating notifications; acknowledge, resolve, silence | 3 |
| Silences | Create and expire | 3 |
| Overview | Volume, delivery success, noisiest sources, failing connections | 3 |
| Escalation view | Chain progress per notification | 4 |

Across all screens: command palette, and layouts that work on a phone for
acknowledging and silencing.

## 8. Auth, secrets and operations

### Auth and secrets

- Local users with email and password. The first-run screen creates the admin.
  All users are admins in v1. The auth layer is an interface with one
  implementation, so a single sign-on provider can be added.
- Three credentials: browser sessions, admin API tokens, per-source ingest
  keys. Keys and tokens are stored hashed and shown once.
- Connection credentials are encrypted with the instance secret key and never
  returned by the API. If no key is supplied, one is generated into the data
  directory on first run.

### Operations

- Static binaries for Linux and macOS on x86-64 and ARM, a container image and
  a compose example from milestone 1. A Helm chart before the public launch.
- Migrations run on start. A binary older than the database schema refuses to
  start.
- Health and readiness endpoints, Prometheus metrics (queue depth, delivery
  latency, failures by connection), structured logs.
- `ramzes backup` for SQLite. Postgres uses its own tooling.
- `ramzes export` and `ramzes import` for the YAML config.
- On shutdown, workers stop claiming and finish in-flight work.
- Ingest has a body size limit and a per-source rate limit that answers 429.
- Persistent delivery failures raise events on a built-in system topic, which
  is routable like any other.

## 9. Error handling

| Failure | Behaviour |
|---|---|
| Invalid event or failed mapping | 4xx with a specific message; the raw request and error are kept under the source |
| Database unavailable at ingest | 503; nothing is acknowledged |
| Delivery: network error, 5xx, 429 | Retry with backoff and jitter up to a maximum number of attempts and age, then `dead` |
| Delivery: rejected (bad credentials, invalid request) | `failed` immediately with the service's response |
| Template fails to render | Send with the connector's default template; flag a warning on the delivery |
| Route expression errors | Route counts as not matching; the error is shown on the route |
| Crash mid-delivery | Lease expires; the delivery is retried |
| Connection failing repeatedly | Paused and probed periodically (milestone 3); until then, per-delivery backoff only |

## 10. Monetization seams

Nothing paid is built before launch. These three seams are built so that paid
features can be added without a rewrite:

- `workspace_id` on every table (team scoping, hosted version).
- Auth behind an interface (SSO).
- Storage behind an interface (multi-node operation on Postgres).

Planned split, to be confirmed before the enterprise edition is started:

| Free | Paid |
|---|---|
| Ingest, routing, all shaping features | SSO (OIDC / SAML) |
| All connectors and source adapters | Role-based access and per-team scoping |
| The full web UI, replay, config export/import | Audit log, long retention and export |
| SQLite or Postgres, single node, local users | Multi-node high availability |
| | Hosted version, support, commercial licence without AGPL obligations |

## 11. Testing

- **Core logic:** table-driven unit tests for matching and shaping against a
  fake clock.
- **Storage:** one contract test suite run against SQLite and Postgres in CI.
- **Connectors and adapters:** tests against a local mock server with
  snapshots of the exact request produced. Declarative definitions are
  schema-validated and snapshot-tested the same way.
- **End to end:** start the real binary, post an event, assert the mock
  receiver got it. Includes a test that kills the process mid-delivery and
  checks the message arrives after restart.
- **UI:** component tests and browser tests for first-run setup, the event
  journey and the route tester.
- **Real services:** a manual check against live Slack, Teams and the other
  launch channels before each release.
- Implementation is test-first.

## 12. Milestones

| # | Name | Contents |
|---|---|---|
| 1 | Core | Workspace skeleton; storage interface on SQLite and Postgres; native ingest; routes with structured matching; route and delivery workers; retries, dead-letter, replay; Slack (incoming webhook) and generic webhook; auth; first-run setup, live feed, event journey, failures, basic route / connection / source / settings screens; binaries, container image, compose example |
| 2 | Breadth | Declarative connector and adapter formats; remaining chat, email and push connectors; Alertmanager, Grafana, GitHub, GitLab, Sentry and any-JSON sources; payload expressions; templates and their editor; connector catalogue; mapping editor; route tester; config history, export/import; Helm chart |
| 3 | Noise control | Timer worker; silences, dedup, grouping, quiet hours, rate limit; resolved events; connection pausing; notifications, silences and overview screens; system topic |
| 4 | Urgency | Escalation chains; acknowledgement by UI, API and signed link; PagerDuty and Opsgenie; Slack app mode with buttons; escalation view |

The public launch follows milestone 4.

The full schema in section 4 is created in milestone 1, including
`notifications` (one per delivery target until grouping exists) and the event
`status` column. Until milestone 3, a resolved event is routed like any other
event.

## 13. To verify during planning

These are checks against current documentation, not open design questions.

- That the template engine (MiniJinja) evaluates standalone expressions
  suitably for route matching and mappings. If not, the fallback is a
  dedicated expression language (CEL).
- The Rust database library and how it serves both SQLite and Postgres behind
  one interface.
- The UI component and styling libraries.
- Per connector, whether its current API can be expressed declaratively.

## 14. Open outside this design

- Demand validation (open question Q4) and a fresh competitor scan
  (`docs/02-landscape.md` is unverified). Neither blocks milestone 1, but the
  positioning in the README should not be published before the scan.
