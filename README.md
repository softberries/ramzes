# ramzes

Self-hosted notification router for internal and operational events.

Services send events to ramzes over HTTP. ramzes stores every event, matches it
against routes, delivers it to Slack or a webhook with retries, and shows in a
web UI what happened to each one and why.

It is one binary with an embedded database and an embedded web UI.

**Status:** milestone 1 of 4 ("Core"). The parts listed under
[What works today](#what-works-today) are built and tested. The rest of the
design is in [Roadmap](#roadmap). There has been no release yet.

## Quickstart

With Docker, from a checkout of this repository:

```sh
docker compose -f deploy/compose.yaml up --build
```

Open <http://localhost:7269>. The setup screen walks through five steps: create
your account, name a source, send a test event, connect a channel, and route
events to it.

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
| `dedup_key` | no | Up to 200 characters. Stored; not acted on until milestone 3 |
| `ts` | no | RFC 3339 time the event occurred. Defaults to the time of receipt |

- Send an array of up to 1000 events to store them in one request.
- Send an `Idempotency-Key` header to make a retry safe: the same key from the
  same source returns the original ids and stores nothing.
- A request body may be up to 1 MiB. A source may send 6000 events per minute;
  beyond that the answer is `429` with `Retry-After`.

## What works today

- **Ingest** over HTTP with per-source keys, batches and idempotency keys.
- **Routes** matched on topic pattern, minimum severity, source and labels
  (equals, regular expression, present). Routes are ordered; a route can stop
  evaluation of the ones below it.
- **Delivery** to Slack (incoming webhook) and to any URL as a signed JSON
  webhook, with retries (5 s doubling to 1 h, up to 8 attempts within 24 h;
  a delivery older than 24 h is given up).
  Delivery is at-least-once.
- **Web UI:** first-run setup, a live feed with filters, a per-event journey
  (routes matched, each delivery attempt with the service's answer), failed
  deliveries grouped by error with bulk resend, and pages for routes,
  connections, sources and settings. Dark by default, with a light theme.
- **Replay:** resend one delivery, or run an event through the current routes
  again.
- **Storage:** SQLite by default, or Postgres. Connection credentials are
  encrypted at rest.
- **Operations:** health and readiness endpoints, Prometheus metrics, retention,
  SQLite backup, graceful shutdown.

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

The TOML file uses the same names with underscores, for example
`listen = "0.0.0.0:7269"`.

ramzes does not terminate TLS. Put it behind a reverse proxy.

Keep `secret.key` (or your `RAMZES_SECRET_KEY`) with your backups: without it
the stored connection credentials cannot be read.

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

## Roadmap

| Milestone | Contents |
|---|---|
| 1. Core | What is listed above |
| 2. Breadth | MS Teams, Discord, email, ntfy, Telegram, Pushover; connectors and inbound adapters defined as data; inbound webhooks from Alertmanager, Grafana, GitHub, GitLab and Sentry; message templates; a route tester; config history and export/import |
| 3. Noise control | Silences, deduplication, grouping and digests, quiet hours, rate limits |
| 4. Urgency | Escalation, acknowledgement, PagerDuty and Opsgenie, Slack buttons |

The full design is in
[docs/superpowers/specs/2026-10-13-ramzes-design.md](docs/superpowers/specs/2026-10-13-ramzes-design.md).

## Licence

The core is licensed under the [GNU Affero General Public License v3.0](LICENSE)
(`AGPL-3.0-only`). See [LICENSING.md](LICENSING.md).
