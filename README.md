# ramzes

Self-hosted notification router for internal and operational events.

Services send events to ramzes over HTTP. ramzes stores every event, matches it
against routes, delivers it to Slack or a webhook with retries, and shows in a
web UI what happened to each one and why.

It is one binary with an embedded database and an embedded web UI.

**Status:** all four milestones of the design are built and covered by
automated tests. Nothing has been released, and no connector has been run
against the live service it talks to yet; see [What was verified](#what-was-verified).

## Quickstart

With Docker, from a checkout of this repository:

```sh
docker compose -f deploy/compose.yaml up --build
```

Open <http://localhost:7269>. The setup screen walks through five steps: create
your account, name a source, send a test event, connect a channel, and route
events to it.

On Kubernetes there is a Helm chart in `deploy/helm/ramzes` (one replica, SQLite
on a volume or Postgres from a Secret).

From source (needs Rust 1.98 and Node 24):

```sh
make build
./target/release/ramzes
```

This creates `./data/ramzes.db` and `./data/secret.key` and listens on
`127.0.0.1:7269`.

## Sending an event

Create a source in the UI to get a key, then:

```sh
curl -X POST http://localhost:7269/v1/events \
  -H "Authorization: Bearer <source key>" \
  -H "Content-Type: application/json" \
  -d '{"topic": "orders.failed", "title": "Order 7 failed", "severity": "error",
       "labels": {"env": "prod"}, "payload": {"order": 7}}'
```

The answer is `202` with the ids of the stored events. ramzes replies only
after the events are written to the database.

| Field | Required | Notes |
|---|---|---|
| `topic` | yes | 1–200 characters: letters, digits and `. _ : / -` |
| `title` | yes | 1–500 characters |
| `severity` | no | `debug`, `info` (default), `warning`, `error`, `critical` |
| `status` | no | `firing` (default) or `resolved` |
| `body` | no | Up to 20 000 characters |
| `labels` | no | Up to 64 string pairs |
| `payload` | no | Any JSON, kept as sent |
| `dedup_key` | no | Up to 200 characters. Identifies what the event is about: dedup uses it, and a later `resolved` event with the same key closes the notification |
| `ts` | no | RFC 3339 time the event occurred. Defaults to the time of receipt |

- Send an array of up to 1000 events to store them in one request.
- Send an `Idempotency-Key` header to make a retry safe: the same key from the
  same source returns the original ids and stores nothing.
- A request body may be up to 1 MiB. A source may send 6000 events per minute;
  beyond that the answer is `429` with `Retry-After`.

## What it does

### Receiving

- **Its own API**: `POST /v1/events` with per-source keys, batches and
  idempotency keys.
- **Other tools' webhooks**, each source at its own address `/v1/in/<source id>`:
  Alertmanager, Grafana, GitHub, GitLab and Sentry are built in.
- **Any JSON**: a source with a mapping you write in the UI (expressions over
  the payload and headers), previewed against the source's recent requests.

| Source kind | How a request is verified |
|---|---|
| ramzes API, Alertmanager, Grafana | `Authorization: Bearer <source key>` |
| GitHub | `X-Hub-Signature-256` (HMAC-SHA256 of the body with the source key as the webhook secret) |
| GitLab | `X-Gitlab-Token` equal to the source key. GitLab's newer signing token is not checked |
| Sentry | `Sentry-Hook-Signature`; you give ramzes the client secret Sentry generated |
| Any JSON | Bearer by default; the mapping can choose a header, a URL parameter or an HMAC header |

### Routing

- Routes are ordered. Each matches on topic pattern, minimum severity, source,
  labels (equals, regular expression, present) and an optional expression over
  the event, for example `payload.amount > 100 and labels.env == 'prod'`.
- A route can stop evaluation of the ones below it.
- The **route tester** shows what every route would do with a stored or sample
  event, including routes you have not saved, without sending anything.

### Noise control

Each route can carry a policy:

| Step | What it does |
|---|---|
| Dedup | Events with the same key as an open notification are counted, not sent again, for a window you choose |
| Grouping | Events are collected for a wait and sent as one message; optionally sent again while still open |
| Quiet hours | Notifications are held during a daily period in a time zone you name, and sent as one digest when it ends; a severity can bypass |
| Rate limit | Beyond N notifications per window the rest are held and sent as one summary |

Besides the policy:

- **Silences** suppress matching events for a period, on every route.
- A **resolved** event closes the open notification with the same key and tells
  everyone who had been told.
- A connection that fails five attempts in a row is **paused** (1 minute,
  doubling to 15) and resumed when it answers again.
- Deliveries that fail for good, and paused connections, raise events on the
  topics `ramzes.delivery.failed` and `ramzes.connection.paused`, which you can
  route like any other.

### Urgency

- A route can have an **escalation chain**: after a delay with no
  acknowledgement, tell someone else; then someone else.
- **Acknowledge** from the UI, the API, a link in the message, or a button in
  Slack. The link opens a page and acknowledges only when its button is pressed.

### Delivering

| Channel | How |
|---|---|
| Slack | Incoming webhook, or your own Slack app (buttons, threads, message updates) |
| Microsoft Teams | Workflows webhook (Adaptive Card) |
| Discord | Channel webhook |
| Email | SMTP |
| ntfy, Telegram, Pushover | Their HTTP APIs |
| PagerDuty | Events API v2: trigger, and resolve when the event is resolved |
| Opsgenie | Alert API: create, and close when resolved. Atlassian shuts Opsgenie down on 5 April 2027 |
| Webhook | Signed JSON to any URL |
| Your own | A YAML definition of one HTTP request, added in the UI |

- **Templates** word the title and body of a message per route target; a
  preview shows the exact request a connection would send.
- Retries: 5 s doubling to 1 h, up to 8 attempts within 24 h. Delivery is
  at-least-once.
- **Replay** resends one delivery, or runs an event through the current routes
  again.

### Seeing what happened

A web UI embedded in the binary: an overview, a live feed, the journey of each
event (routes matched, shaping decisions, each attempt with the service's
answer, the message as it was worded), open notifications, failures with bulk
resend, silences, and pages for routes, templates, connections, sources and
settings. Dark by default, with a light theme.

### Configuration as data

- Every change to routes, templates, connections, connectors and settings is
  recorded; any past state can be compared and restored.
- `ramzes export` writes the configuration as YAML without credentials or keys;
  `ramzes import` applies one and keeps the stored credentials.

## Configuration

Each setting can be given as a flag, an environment variable, or in a TOML file
passed with `--config`. A flag or environment variable overrides the file.

| Flag | Environment variable | Default | Meaning |
|---|---|---|---|
| `--listen` | `RAMZES_LISTEN` | `127.0.0.1:7269` | Address to listen on |
| `--data-dir` | `RAMZES_DATA_DIR` | `./data` | Where the SQLite database and the generated secret key live |
| `--database-url` | `RAMZES_DATABASE_URL` | SQLite in the data directory | `sqlite://path` or `postgres://user:pass@host/db` |
| `--secret-key` | `RAMZES_SECRET_KEY` | generated into `<data-dir>/secret.key` | Base64 of 32 bytes; encrypts stored credentials |
| `--public-url` | `RAMZES_PUBLIC_URL` | none | Address people reach ramzes at, for the link in each message |
| `--secure-cookies true` | `RAMZES_SECURE_COOKIES=true` | `false` | Mark the session cookie `Secure`; set when serving over HTTPS |
| `--block-private-addresses true` | `RAMZES_BLOCK_PRIVATE_ADDRESSES=true` | `false` | Refuse to deliver to loopback, private and link-local addresses |
| `--metrics-token` | `RAMZES_METRICS_TOKEN` | none | Ask for this token on `/metrics`, as `Authorization: Bearer <token>` |

The TOML file uses the same names with underscores, for example
`listen = "0.0.0.0:7269"`.

ramzes does not terminate TLS. Put it behind a reverse proxy.

Set the public URL (here or under Settings) if you want messages to link back
to ramzes and carry the acknowledgement link.

Keep `secret.key` (or your `RAMZES_SECRET_KEY`) with your backups: without it
the stored connection credentials cannot be read, and acknowledgement links
that were already sent stop working.

Logging: `RAMZES_LOG` sets the level (default `info`), `RAMZES_LOG_FORMAT=json`
switches to JSON lines.

## Verifying a webhook

When a webhook connection has a signing secret, each request carries
`X-Ramzes-Timestamp` (unix milliseconds) and `X-Ramzes-Signature`. The signature
is `sha256=` followed by the hex HMAC-SHA256, keyed with the secret, of the
timestamp, a dot, and the raw request body.

## Operating it

| Endpoint | Purpose |
|---|---|
| `GET /healthz` | The process is up |
| `GET /readyz` | The database answers |
| `GET /metrics` | Prometheus metrics: events ingested, delivery attempts by connector and outcome, attempt duration, queue depths |
| `GET /api/openapi.json` | OpenAPI description of the admin API |

- `ramzes backup <file>` writes a consistent copy of the SQLite database to a
  new file. For Postgres use `pg_dump`.
- `ramzes export [file]` and `ramzes import <file>` move the configuration
  between instances.
- `/metrics` is open unless a metrics token is set. Set one, or do not expose it to the internet.
- Sign-in is refused for 15 minutes after 10 wrong passwords for an email.
- ramzes runs as one instance. It does not yet coordinate several.
- Events are kept for 30 days by default. Change this, or set it per topic,
  under Settings.
- Admin API tokens for automation are created under Settings and sent as
  `Authorization: Bearer <token>`.

## Development

```sh
make test       # Rust tests on SQLite
make test-pg    # the same tests on SQLite and on a throwaway Postgres in Docker
make ui-test    # lint, type check and unit tests of the web UI
make e2e        # browser tests against the real binary
make lint       # rustfmt and clippy
```

To work on the UI with hot reload, run `make run` and, in another terminal,
`npm --prefix ui run dev`.

## What was verified

| Claim | How it was checked |
|---|---|
| Every feature listed above | Automated tests, last run on 13 October 2026: 508 Rust tests on SQLite and on Postgres 18 (including 17 that start the real binary), 275 UI unit and component tests, 21 browser tests against the real binary |
| Quickstart with Docker | The image was built from this tree and started: health check, the UI and the setup endpoint answered, and it exited with status 0 on SIGTERM. The compose files were last run for milestone 1 |
| Quickstart from source | `make build` was last run by hand for milestone 1; the test suites build the same binary |
| Request formats of Slack, Teams, Discord, ntfy, Telegram, Pushover, PagerDuty, Opsgenie | Written from each service's documentation as read on 13 October 2026, pinned by snapshot tests. **Not sent to the live services** |
| Payload shapes of Alertmanager, Grafana, GitHub, GitLab, Sentry | From their documentation; GitHub's signature is checked against GitHub's published example. **Not received from the live tools** |
| Email | Sent to a test SMTP server inside the test suite. Not sent through a real provider; TLS modes are untested |
| Opsgenie shutdown on 5 April 2027 | Atlassian's migration page, read on 13 October 2026 |
| Helm chart | `helm lint` and `helm template`. Not installed on a cluster |
| Release workflow | Never run |

## What is not built

- Several instances working together (high availability).
- Single sign-on, roles, an audit log. Every user can change everything.
- Full-text search; the feed filters by time, source, topic, severity, label,
  outcome and a substring of the title.
- Pushing an acknowledgement made in ramzes to PagerDuty or Opsgenie.
- GitLab's signing-token verification.
- A memory limit for templates and expressions. They are stopped after a fixed
  amount of work, but a signed-in user can still write one that builds a very
  large string. Everyone who can sign in is an administrator; do not give
  accounts or API tokens to people you would not trust with the server.

The design is in
[docs/superpowers/specs/2026-10-13-ramzes-design.md](docs/superpowers/specs/2026-10-13-ramzes-design.md).

## Licence

The core is licensed under the [GNU Affero General Public License v3.0](LICENSE)
(`AGPL-3.0-only`). See [LICENSING.md](LICENSING.md).
