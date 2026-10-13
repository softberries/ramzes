# ramzes Milestone 1 (Core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A single `ramzes` binary that accepts events over HTTP, stores them, routes them by structured rules, delivers them to Slack (incoming webhook) and a signed generic webhook with retries, and shows the whole path in an embedded web UI.

**Architecture:** The database is the queue. Ingest writes events with `routed_at` null; a route worker claims them by lease, evaluates routes with pure functions from `ramzes-core`, and writes notifications and deliveries; a delivery worker claims due deliveries, sends through a connector and records each attempt. The same SQL runs on SQLite and Postgres behind one `Store` type.

**Tech Stack:** Rust 1.98 (edition 2024), Tokio, Axum 0.8, sqlx 0.9 (SQLite + Postgres, runtime queries), utoipa + utoipa-axum (OpenAPI), reqwest, argon2, chacha20poly1305, rust-embed, prometheus-client. UI: SvelteKit (Svelte 5) as a static single-page app, TypeScript, Tailwind CSS 4, Vitest, Playwright.

**Spec:** `docs/superpowers/specs/2026-10-13-ramzes-design.md`

## Global Constraints

- Licence `AGPL-3.0-only` in every `Cargo.toml` and `ui/package.json`.
- No `#[allow(dead_code)]`; remove unused code instead.
- Dependency versions are resolved with `cargo add` / `npm install` at execution time, not typed from memory. Where Tayga (`../tayga/Cargo.toml`) pins a crate, use the same version.
- One migration set for both databases. Allowed column types: `TEXT`, `BIGINT`. Booleans are `BIGINT` 0/1. Timestamps are `BIGINT` unix milliseconds. Ids are UUIDv7 as `TEXT`. JSON is `TEXT`. Binary data is base64 `TEXT`.
- SQL is written once with `$N` placeholders; the store rewrites `$N` to `?N` for SQLite.
- Every table except `workspaces` has `workspace_id`.
- Nothing is acknowledged to a sender before it is committed.
- Pure logic (matching, routing, backoff, message building) lives in `ramzes-core` and takes the current time as an argument.
- Delivery is at-least-once.
- UI colours come only from theme tokens; dark is the default and light must work.
- Test-first: each task writes its failing tests before the implementation.
- Commit after every task with the session attribution trailer.

## Review Focus

1. **A connection is deleted while deliveries for it are pending.** The deliveries must end as `failed` with a reason naming the missing connection, not loop or panic. Test in Task 11.
2. **The process dies with deliveries `in_progress`.** After restart and lease expiry they are claimed again and delivered. Test in Tasks 8 and 15.
3. **Malformed ingest input:** invalid JSON, a body over the size limit, a batch over 1000 events, a label value that is not a string, an unknown severity. Each returns a specific 4xx, never a 500. Tests in Task 12.
4. **The receiving service hangs.** A connector call that exceeds the timeout is a retryable failure and frees the worker. Test in Task 10.
5. **A client retries a request it already sent.** The same `Idempotency-Key` from the same source returns the original event ids and creates nothing. Tests in Tasks 6 and 12.

---

## File Structure

```
Cargo.toml                         workspace, shared deps and lints
LICENSE, LICENSING.md              AGPL-3.0 text, licensing summary
Makefile                           test, test-pg, ui, build, run
crates/
  ramzes-core/src/
    lib.rs                         re-exports
    ids.rs                         new_id() (UUIDv7)
    event.rs                       Severity, EventStatus, NewEvent, Event, validation
    matching.rs                    RouteMatch, LabelCond, topic glob, matches()
    routing.rs                     Route, Target, route_event()
    message.rs                     Message, build_message()
    backoff.rs                     RetryPolicy, next_attempt()
  ramzes-store/
    migrations/0001_init.sql       full schema
    src/lib.rs                     Store, Db, connect(), sql rewrite, with_db! macro
    src/{sources,events,connections,routes,notifications,deliveries,auth,settings}.rs
    tests/contract.rs              one suite, run against SQLite and Postgres
  ramzes-connectors/src/
    lib.rs                         Connector trait, Registry, SendOutcome, descriptors
    http.rs                        shared reqwest client, outcome classification
    webhook.rs                     signed generic webhook
    slack.rs                       Slack incoming webhook
  ramzes-engine/src/
    lib.rs                         Engine, Wake, run()
    route_worker.rs                claim + route + write
    delivery_worker.rs             claim + send + record
    replay.rs                      replay_delivery, replay_event
    retention.rs                   retention sweep
  ramzes-ingest/src/lib.rs         POST /v1/events router, limits, rate limiter
  ramzes-api/src/
    lib.rs                         router(), AppState, ApiError
    crypto.rs                      SecretKey, seal/open
    auth.rs                        setup, login, logout, me, extractors
    sources.rs connections.rs routes.rs events.rs failures.rs settings.rs stream.rs
  ramzes-server/
    build.rs                       ensures ui/build exists
    src/main.rs                    CLI: serve, backup, openapi
    src/config.rs                  Config from env / TOML
    src/serve.rs                   wiring, shutdown
    src/ui.rs                      embedded assets with SPA fallback
    src/ops.rs                     /healthz /readyz /metrics
    tests/e2e.rs                   real binary against a mock receiver
ui/                                SvelteKit app (see Tasks 16-20)
deploy/Dockerfile, deploy/compose.yaml, deploy/compose.postgres.yaml
.github/workflows/ci.yml, release.yml
```

---

### Task 1: Workspace skeleton

**Files:** `Cargo.toml`, `crates/*/Cargo.toml`, `crates/*/src/lib.rs` (empty), `crates/ramzes-server/src/main.rs`, `LICENSE`, `LICENSING.md`, `Makefile`, `rust-toolchain.toml`, `.github/workflows/ci.yml`

**Produces:** seven crates that compile; `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo nextest run --workspace` all pass.

- [ ] Create the workspace `Cargo.toml` (resolver 3, edition 2024, `rust-version = "1.98"`, `license = "AGPL-3.0-only"`, `[workspace.lints.clippy] all = "deny"`), and the seven crates with `cargo new --lib` / `--bin`.
- [ ] Copy `LICENSE` from `../tayga/LICENSE` (identical AGPL-3.0 text). Write `LICENSING.md`: core AGPL-3.0-only, commercial licence on request at hello@softberries.dev, CLA to be set up before outside contributions are accepted.
- [ ] `Makefile` targets: `test` (nextest, SQLite only), `test-pg` (starts `postgres:18` in Docker on port 55432, exports `RAMZES_TEST_POSTGRES_URL`, runs nextest, removes the container), `ui`, `build`, `run`.
- [ ] `ci.yml`: fmt, clippy, nextest with a Postgres service and `RAMZES_TEST_POSTGRES_URL` set; UI lint, typecheck, unit tests.
- [ ] Run fmt, clippy, nextest. Expected: pass with zero tests.
- [ ] Commit: `chore: workspace skeleton, licence, CI`.

### Task 2: Core domain types

**Files:** `crates/ramzes-core/src/{lib,ids,event}.rs`

**Produces:**
```rust
pub fn new_id() -> String;                                   // UUIDv7, lowercase hyphenated
pub enum Severity { Debug, Info, Warning, Error, Critical }  // serde lowercase; Ord; as_i64()/from_i64()
pub enum EventStatus { Firing, Resolved }                    // serde lowercase; as_str()/parse()
pub struct NewEvent { topic, title, severity, status, body: Option<String>,
    labels: BTreeMap<String,String>, payload: serde_json::Value,
    dedup_key: Option<String>, ts: Option<String> }           // Deserialize, deny_unknown_fields
pub struct Event { id, workspace_id, source_id, topic, severity, status, title, body,
    labels, payload, dedup_key, occurred_at: i64, received_at: i64 }
pub enum ValidationError { .. }                               // thiserror, one variant per rule
impl NewEvent { pub fn validate(&self) -> Result<(), ValidationError>;
                pub fn occurred_at(&self, now_ms: i64) -> Result<i64, ValidationError>; }
```
Rules: topic 1–200 chars matching `[A-Za-z0-9._:/-]+`; title 1–500 chars; body ≤ 20 000 chars; ≤ 64 labels; label key 1–100 chars `[A-Za-z0-9._-]+`; label value ≤ 1000 chars; `ts` is RFC 3339.

- [ ] Tests: ids are unique and sort by creation order; severity orders `Debug < Critical` and round-trips through `i64` and JSON; each validation rule has one accepting and one rejecting case; `occurred_at` parses `2026-10-13T09:41:07Z` to `1791884467000`, defaults to `now_ms`, rejects `"yesterday"`; unknown JSON fields are rejected; a non-string label value fails to deserialize.
- [ ] Implement. Run `cargo nextest run -p ramzes-core`. Commit: `feat(core): event types and validation`.

### Task 3: Matching and routing

**Files:** `crates/ramzes-core/src/{matching,routing}.rs`

**Produces:**
```rust
pub struct RouteMatch { topic: Option<String>, min_severity: Option<Severity>,
    source_id: Option<String>, labels: Vec<LabelCond> }       // serde, default = matches everything
pub enum LabelCond { Equals{key,value}, Matches{key,pattern}, Exists{key} }  // serde tag = "op"
impl RouteMatch { pub fn validate(&self) -> Result<(), MatchError>;           // bad regex / glob
                  pub fn matches(&self, e: &Event) -> bool; }
pub struct Target { connection_id: String, destination: Option<String> }
pub struct Route { id, name, position: i64, enabled: bool, stop: bool,
    r#match: RouteMatch, targets: Vec<Target> }
pub struct Matched<'a> { pub route: &'a Route, pub target: &'a Target }
pub fn route_event<'a>(e: &Event, routes: &'a [Route]) -> Vec<Matched<'a>>;
```
Topic glob: `*` matches any run of characters, everything else is literal, the whole topic must match. `Matches` uses an anchored-by-the-user regex with a 10 000-byte compiled size limit; a pattern that fails to compile matches nothing.

- [ ] Tests: glob `orders.*` matches `orders.failed`, not `xorders.failed`; `*` matches all; literal dots are literal; `min_severity: error` rejects `warning`, accepts `critical`; each label condition true and false; empty match accepts everything; `route_event` evaluates in `position` order, skips disabled routes, returns every target of every matching route, and stops after a matching route with `stop`; a non-matching `stop` route does not stop; no match returns empty.
- [ ] Implement. Commit: `feat(core): route matching`.

### Task 4: Canonical message and retry policy

**Files:** `crates/ramzes-core/src/{message,backoff}.rs`

**Produces:**
```rust
pub struct Message { title, body: Option<String>, severity: Severity, status: EventStatus,
    topic: String, fields: Vec<(String,String)>, link: Option<String>, event_ids: Vec<String> }
pub fn build_message(e: &Event, public_url: Option<&str>) -> Message;  // fields = labels in key order; link = {public_url}/events/{id}
pub struct RetryPolicy { base_ms: i64, max_ms: i64, max_attempts: i64, max_age_ms: i64 }
impl Default for RetryPolicy  // 5 s, 1 h, 8 attempts, 24 h
pub enum Next { At(i64), GiveUp }
pub fn next_attempt(p: &RetryPolicy, attempts_done: i64, created_at: i64, now_ms: i64,
                    retry_after_ms: Option<i64>, jitter: f64 /* 0.0..1.0 */) -> Next;
```
Delay = `min(max_ms, base_ms * 2^(attempts_done-1))`, scaled by `0.8 + 0.4 * jitter`; a `retry_after_ms` larger than the delay replaces it; `GiveUp` when `attempts_done >= max_attempts` or the next time is past `created_at + max_age_ms`.

- [ ] Tests: delays for attempts 1, 2, 3 at jitter 0.5 are 5 s, 10 s, 20 s; cap at 1 h; jitter 0.0 and 1.0 give 0.8× and 1.2×; retry-after wins when larger, loses when smaller; gives up at attempt 8 and past max age; `build_message` carries labels as sorted fields and builds the link only when a public URL is set.
- [ ] Implement. Commit: `feat(core): canonical message and retry policy`.

### Task 5: Store foundation and schema

**Files:** `crates/ramzes-store/migrations/0001_init.sql`, `src/lib.rs`, `src/settings.rs`, `tests/contract.rs`

**Produces:**
```rust
pub struct Store { /* Db enum: Sqlite(SqlitePool) | Postgres(PgPool) */ }
pub enum StoreError { NotFound, Conflict(String), Db(sqlx::Error), Migrate(..), Data(String) }
impl Store {
    pub async fn connect(url: &str) -> Result<Store, StoreError>;   // sqlite: create_if_missing, WAL, foreign keys, busy_timeout 5 s
    pub async fn migrate(&self) -> Result<(), StoreError>;          // refuses when the database is ahead of the binary
    pub async fn ensure_workspace(&self, now_ms: i64) -> Result<String, StoreError>;  // returns the single workspace id
    pub async fn ping(&self) -> Result<(), StoreError>;
    pub fn backend(&self) -> &'static str;                          // "sqlite" | "postgres"
    pub async fn get_setting(&self, ws: &str, key: &str) -> Result<Option<String>, StoreError>;
    pub async fn set_setting(&self, ws: &str, key: &str, value: &str) -> Result<(), StoreError>;
}
pub(crate) fn sql(backend, query: &'static str) -> Cow<'static, str>;   // $N -> ?N for SQLite
```

Schema (`0001_init.sql`), all columns `TEXT` or `BIGINT`:

```sql
CREATE TABLE workspaces (id TEXT PRIMARY KEY, name TEXT NOT NULL, created_at BIGINT NOT NULL);
CREATE TABLE settings (workspace_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (workspace_id, key));
CREATE TABLE users (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, email TEXT NOT NULL UNIQUE, password_hash TEXT NOT NULL, created_at BIGINT NOT NULL);
CREATE TABLE sessions (token_hash TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, user_id TEXT NOT NULL, created_at BIGINT NOT NULL, expires_at BIGINT NOT NULL);
CREATE TABLE api_tokens (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, token_hash TEXT NOT NULL UNIQUE, token_prefix TEXT NOT NULL, created_by TEXT NOT NULL, created_at BIGINT NOT NULL, last_used_at BIGINT);
CREATE TABLE sources (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL, key_hash TEXT NOT NULL UNIQUE, key_prefix TEXT NOT NULL, mapping TEXT, created_at BIGINT NOT NULL);
CREATE TABLE source_requests (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, source_id TEXT NOT NULL, received_at BIGINT NOT NULL, body TEXT NOT NULL, error TEXT);
CREATE INDEX source_requests_source ON source_requests (source_id, id);
CREATE TABLE events (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, source_id TEXT NOT NULL, topic TEXT NOT NULL, severity BIGINT NOT NULL, status TEXT NOT NULL, title TEXT NOT NULL, body TEXT, payload TEXT NOT NULL, dedup_key TEXT, idempotency_key TEXT, occurred_at BIGINT NOT NULL, received_at BIGINT NOT NULL, routed_at BIGINT, claimed_until BIGINT, route_count BIGINT NOT NULL DEFAULT 0);
CREATE UNIQUE INDEX events_idempotency ON events (source_id, idempotency_key);
CREATE INDEX events_unrouted ON events (id) WHERE routed_at IS NULL;
CREATE INDEX events_topic ON events (workspace_id, topic, id);
CREATE INDEX events_source ON events (workspace_id, source_id, id);
CREATE INDEX events_received ON events (received_at);
CREATE TABLE event_labels (event_id TEXT NOT NULL, workspace_id TEXT NOT NULL, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY (event_id, key));
CREATE INDEX event_labels_kv ON event_labels (key, value, event_id);
CREATE TABLE connections (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, kind TEXT NOT NULL, name TEXT NOT NULL, settings TEXT NOT NULL, credentials TEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE connector_definitions (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, definition TEXT NOT NULL, created_at BIGINT NOT NULL);
CREATE TABLE templates (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, connector_kind TEXT NOT NULL, name TEXT NOT NULL, body TEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE routes (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL, position BIGINT NOT NULL, enabled BIGINT NOT NULL, stop BIGINT NOT NULL, match_json TEXT NOT NULL, targets_json TEXT NOT NULL, shaping_json TEXT NOT NULL, escalation_json TEXT NOT NULL, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE silences (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, match_json TEXT NOT NULL, starts_at BIGINT NOT NULL, ends_at BIGINT NOT NULL, author TEXT NOT NULL, comment TEXT, created_at BIGINT NOT NULL);
CREATE TABLE notifications (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, route_id TEXT NOT NULL, route_name TEXT NOT NULL, connection_id TEXT NOT NULL, destination TEXT, group_key TEXT, dedup_key TEXT, state TEXT NOT NULL, reason TEXT, event_count BIGINT NOT NULL, replay BIGINT NOT NULL, acked_by TEXT, acked_at BIGINT, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE TABLE notification_events (notification_id TEXT NOT NULL, event_id TEXT NOT NULL, workspace_id TEXT NOT NULL, PRIMARY KEY (notification_id, event_id));
CREATE INDEX notification_events_event ON notification_events (event_id);
CREATE TABLE deliveries (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, notification_id TEXT NOT NULL, connection_id TEXT NOT NULL, destination TEXT, state TEXT NOT NULL, attempts BIGINT NOT NULL, next_attempt_at BIGINT NOT NULL, claimed_until BIGINT, last_error TEXT, external_ref TEXT, created_at BIGINT NOT NULL, updated_at BIGINT NOT NULL);
CREATE INDEX deliveries_due ON deliveries (next_attempt_at) WHERE state IN ('pending', 'in_progress');
CREATE INDEX deliveries_notification ON deliveries (notification_id);
CREATE INDEX deliveries_state ON deliveries (workspace_id, state, id);
CREATE TABLE delivery_attempts (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, delivery_id TEXT NOT NULL, started_at BIGINT NOT NULL, duration_ms BIGINT NOT NULL, outcome TEXT NOT NULL, status BIGINT, detail TEXT);
CREATE INDEX delivery_attempts_delivery ON delivery_attempts (delivery_id, id);
CREATE TABLE timers (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, kind TEXT NOT NULL, due_at BIGINT NOT NULL, claimed_until BIGINT, notification_id TEXT NOT NULL);
CREATE TABLE config_revisions (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, author TEXT NOT NULL, created_at BIGINT NOT NULL, snapshot TEXT NOT NULL);
```

`settings` is not in the spec's table list; it holds retention and the public URL.

Contract harness: `tests/contract.rs` defines each test as `async fn name(store: Store)`, and a macro expands it into `sqlite_name` (temp file) and `postgres_name` (fresh schema per test in the database at `RAMZES_TEST_POSTGRES_URL`; prints a skip notice and returns when the variable is unset).

- [ ] Tests: `sql` rewrites `$1 … $12` and leaves Postgres text unchanged; `migrate` twice is a no-op; `ensure_workspace` returns the same id on a second call; settings round-trip and overwrite; `ping` succeeds.
- [ ] Implement. Run `make test-pg`. Expected: every test passes under both prefixes.
- [ ] Commit: `feat(store): schema and dual-database foundation`.

### Task 6: Store — sources and events

**Files:** `crates/ramzes-store/src/{sources,events}.rs`, `tests/contract.rs`

**Produces:**
```rust
pub struct Source { id, workspace_id, name, kind, key_prefix, created_at }
impl Store {
  pub async fn create_source(&self, ws, name, kind, key_hash, key_prefix, now) -> Result<Source>;
  pub async fn list_sources(&self, ws) -> Result<Vec<Source>>;
  pub async fn source_by_key_hash(&self, key_hash) -> Result<Option<Source>>;
  pub async fn rotate_source_key(&self, ws, id, key_hash, key_prefix) -> Result<()>;
  pub async fn delete_source(&self, ws, id) -> Result<()>;
  pub async fn record_source_request(&self, ws, source_id, body, error: Option<&str>, now) -> Result<()>;  // keeps the newest 20 per source
  pub async fn list_source_requests(&self, ws, source_id) -> Result<Vec<SourceRequest>>;

  /// Inserts all events in one transaction. If the idempotency key was already used by this
  /// source, returns the original ids and inserts nothing.
  pub async fn insert_events(&self, events: &[Event], idempotency_key: Option<&str>) -> Result<InsertOutcome>;
  pub async fn get_event(&self, ws, id) -> Result<Option<Event>>;
  pub async fn list_events(&self, ws, f: &EventFilter) -> Result<Vec<EventRow>>;   // newest first, keyset on id
  pub async fn claim_unrouted(&self, now, lease_ms, limit) -> Result<Vec<Event>>;
  pub async fn mark_routed(&self, event_id, route_count, now) -> Result<()>;
  pub async fn delete_events_before(&self, ws, cutoff_ms, topic: Option<&str>, except_topics: &[String], limit) -> Result<u64>;
}
pub struct InsertOutcome { pub ids: Vec<String>, pub duplicate: bool }
pub struct EventFilter { before_id, source_id, topic, min_severity, label: Option<(String,String)>,
    title_contains, since_ms, until_ms, outcome: Option<Outcome>, limit: i64 }
pub enum Outcome { Pending, Delivered, Failed, Unrouted }
pub struct EventRow { #[flatten] event: Event, route_count: i64, routed: bool,
    deliveries_total: i64, deliveries_delivered: i64, deliveries_failed: i64 }
```
Batch idempotency: event *i* of a request stores `"{key}#{i}"`. `delete_events_before` also deletes the events' labels, notification links, notifications left without events, their deliveries and attempts.

- [ ] Tests: source CRUD and lookup by key hash; rotating a key invalidates the old hash; only 20 source requests are kept; inserted events read back equal, with labels and payload; a repeated idempotency key returns the original ids with `duplicate = true` and the row count is unchanged *(Review Focus 5)*; the same key from another source is not a duplicate; each filter narrows correctly, including label and each `Outcome`; keyset paging with `before_id` returns disjoint, ordered pages; `claim_unrouted` returns each event once, returns it again after the lease expires, and never after `mark_routed`; retention deletes old events and every dependent row and respects the per-topic arguments.
- [ ] Implement. `make test-pg`. Commit: `feat(store): sources and events`.

### Task 7: Store — connections and routes

**Files:** `crates/ramzes-store/src/{connections,routes}.rs`, `tests/contract.rs`

**Produces:**
```rust
pub struct Connection { id, workspace_id, kind, name, settings: serde_json::Value, credentials: String /* sealed */, created_at, updated_at }
impl Store {
  pub async fn create_connection(&self, c: &Connection) -> Result<()>;
  pub async fn update_connection(&self, c: &Connection) -> Result<()>;
  pub async fn get_connection(&self, ws, id) -> Result<Option<Connection>>;
  pub async fn list_connections(&self, ws) -> Result<Vec<Connection>>;
  pub async fn delete_connection(&self, ws, id) -> Result<()>;
  pub async fn create_route(&self, ws, r: &Route, now) -> Result<()>;      // appended at the end
  pub async fn update_route(&self, ws, r: &Route, now) -> Result<()>;
  pub async fn delete_route(&self, ws, id) -> Result<()>;
  pub async fn list_routes(&self, ws) -> Result<Vec<Route>>;               // by position
  pub async fn reorder_routes(&self, ws, ids: &[String], now) -> Result<()>;  // Conflict unless ids is a permutation of all route ids
}
```

- [ ] Tests: CRUD round-trips; update of a missing row is `NotFound`; routes list in position order; a new route gets the last position; reorder applies exactly the given order and rejects a list with a missing or unknown id; the route's match and targets survive a round-trip unchanged.
- [ ] Implement. `make test-pg`. Commit: `feat(store): connections and routes`.

### Task 8: Store — notifications, deliveries, attempts

**Files:** `crates/ramzes-store/src/{notifications,deliveries}.rs`, `tests/contract.rs`

**Produces:**
```rust
pub struct NewDispatch { notification_id, delivery_id, route_id, route_name, connection_id, destination: Option<String>, replay: bool }
pub struct Delivery { id, workspace_id, notification_id, connection_id, destination, state: DeliveryState,
    attempts, next_attempt_at, last_error, external_ref, created_at, updated_at }
pub enum DeliveryState { Pending, InProgress, Delivered, Failed, Dead }
pub struct Attempt { id, delivery_id, started_at, duration_ms, outcome: String, status: Option<i64>, detail: Option<String> }
pub enum Finish { Delivered{external_ref: Option<String>}, Retry{at: i64, error: String}, Failed{error: String}, Dead{error: String} }
impl Store {
  /// One transaction: notifications (state "sent"), notification_events, deliveries (pending, due now),
  /// and events.routed_at / route_count.
  pub async fn write_routing(&self, event: &Event, dispatches: &[NewDispatch], mark_routed: bool, now) -> Result<()>;
  pub async fn claim_due_deliveries(&self, now, lease_ms, limit) -> Result<Vec<Delivery>>;
  pub async fn finish_attempt(&self, d: &Delivery, a: &Attempt, f: &Finish, now) -> Result<()>;   // one transaction
  pub async fn notification_events(&self, notification_id) -> Result<Vec<Event>>;
  pub async fn journey(&self, ws, event_id) -> Result<Option<Journey>>;   // event + notifications + deliveries + attempts
  pub async fn list_failures(&self, ws, limit) -> Result<Vec<FailureRow>>; // failed + dead, newest first, with event title and connection name
  pub async fn requeue_delivery(&self, ws, delivery_id, new_id, now) -> Result<Delivery>;  // new pending delivery on the same notification
  pub async fn queue_depth(&self) -> Result<QueueDepth>;                  // unrouted events, pending deliveries
}
```
`claim_due_deliveries` selects rows in `pending` or `in_progress` with `next_attempt_at <= now` and an absent or expired lease, repeats the lease condition in the outer `UPDATE`, sets `state = 'in_progress'` and `claimed_until = now + lease_ms`, and returns the rows.

- [ ] Tests: `write_routing` with two dispatches creates two notifications and two pending deliveries and marks the event routed, all visible together; with zero dispatches it marks the event routed with `route_count = 0`; claiming returns each due delivery once; a delivery not finished is claimable again after the lease *(Review Focus 2)*; a future `next_attempt_at` is not claimed; each `Finish` variant sets the state, attempts, error and next time and stores the attempt; `journey` returns the full tree; `list_failures` contains only failed and dead; `requeue_delivery` adds a pending delivery and leaves the original untouched.
- [ ] Implement. `make test-pg`. Commit: `feat(store): notifications and deliveries`.

### Task 9: Store — users, sessions, tokens

**Files:** `crates/ramzes-store/src/auth.rs`, `tests/contract.rs`

**Produces:**
```rust
pub struct User { id, workspace_id, email, password_hash, created_at }
pub struct ApiToken { id, name, token_prefix, created_by, created_at, last_used_at }
impl Store {
  pub async fn count_users(&self) -> Result<i64>;
  pub async fn create_user(&self, u: &User) -> Result<()>;                 // Conflict on duplicate email
  pub async fn create_first_user(&self, u: &User) -> Result<()>;           // Conflict if any user exists; one transaction
  pub async fn user_by_email(&self, email) -> Result<Option<User>>;
  pub async fn list_users(&self, ws) -> Result<Vec<User>>;
  pub async fn delete_user(&self, ws, id) -> Result<()>;                   // Conflict when it is the last user
  pub async fn create_session(&self, token_hash, ws, user_id, now, expires_at) -> Result<()>;
  pub async fn session_user(&self, token_hash, now) -> Result<Option<User>>;
  pub async fn delete_session(&self, token_hash) -> Result<()>;
  pub async fn create_api_token(&self, ws, t: &ApiToken, token_hash) -> Result<()>;
  pub async fn api_token_workspace(&self, token_hash, now) -> Result<Option<String>>;  // also sets last_used_at
  pub async fn list_api_tokens(&self, ws) -> Result<Vec<ApiToken>>;
  pub async fn delete_api_token(&self, ws, id) -> Result<()>;
}
```

- [ ] Tests: `create_first_user` succeeds once and conflicts the second time; duplicate email conflicts; an expired session returns no user; deleting a session logs it out; the last user cannot be deleted; token lookup works and updates `last_used_at`; a deleted token stops working.
- [ ] Implement. `make test-pg`. Commit: `feat(store): users, sessions, API tokens`.

### Task 10: Connectors

**Files:** `crates/ramzes-connectors/src/{lib,http,webhook,slack}.rs`

**Produces:**
```rust
pub struct ConnectorDescriptor { kind: &'static str, name: &'static str, description: &'static str,
    fields: Vec<Field>, capabilities: Capabilities, destination_label: Option<&'static str> }
pub struct Field { key, label, help, secret: bool, required: bool, kind: FieldKind /* Text | Url */ }
pub struct Capabilities { rich: bool, update: bool, threads: bool, buttons: bool, max_len: usize }
pub struct ConnectionConfig { pub settings: serde_json::Value, pub credentials: serde_json::Value }  // credentials already opened
pub enum SendOutcome { Delivered{ external_ref: Option<String>, status: Option<u16> },
    Retry{ after_ms: Option<i64>, status: Option<u16>, detail: String },
    Permanent{ status: Option<u16>, detail: String } }
#[async_trait] pub trait Connector: Send + Sync {
    fn descriptor(&self) -> ConnectorDescriptor;
    fn validate(&self, c: &ConnectionConfig) -> Result<(), String>;
    async fn send(&self, c: &ConnectionConfig, m: &Message, destination: Option<&str>, now_ms: i64) -> SendOutcome;
}
pub struct Registry;  impl Registry { pub fn builtin(timeout: Duration) -> Self;
    pub fn get(&self, kind: &str) -> Option<Arc<dyn Connector>>; pub fn descriptors(&self) -> Vec<ConnectorDescriptor>; }
pub fn classify(status: u16, retry_after: Option<&str>, body_excerpt: String) -> SendOutcome;
```
`classify`: 2xx delivered; 408, 425, 429 and 5xx retry (with `Retry-After` seconds when present); other 4xx permanent. Network errors and timeouts are `Retry`. Response excerpts are cut to 500 characters.

**Webhook** (`kind = "webhook"`): settings `url`; credential `secret` (optional). Body is JSON `{ "message": Message, "sent_at": now_ms }`. With a secret it adds `X-Ramzes-Timestamp: <now_ms>` and `X-Ramzes-Signature: sha256=<hex HMAC-SHA256 of "<now_ms>.<body>">`.

**Slack** (`kind = "slack"`): credential `webhook_url`. Body is `{ "text": "<fallback>", "blocks": [...] }`: a header block with a severity marker and the title, a section with the body when present, a fields section with up to 10 labels, a context block with topic, status and the link. Text over Slack's limits is truncated with an ellipsis.

- [ ] Tests (local mock server via `wiremock`): `classify` table for 200, 204, 400, 401, 404, 408, 429 with and without `Retry-After`, 500, 503; webhook sends the expected JSON (snapshot with `insta`); the signature verifies against an independent HMAC computation and is absent without a secret; Slack body snapshot for an event with body and labels and for a bare one; a 3 000-character title is truncated; a server that answers after the timeout yields `Retry` within the timeout plus a margin *(Review Focus 4)*; a refused connection yields `Retry`; `validate` rejects a missing or non-http(s) URL.
- [ ] Implement. Commit: `feat(connectors): connector interface, webhook and Slack`.

### Task 11: Engine

**Files:** `crates/ramzes-engine/src/{lib,route_worker,delivery_worker,replay,retention}.rs`

**Produces:**
```rust
pub trait Clock: Send + Sync { fn now_ms(&self) -> i64; }   pub struct SystemClock;
pub trait Opener: Send + Sync { fn open(&self, sealed: &str) -> Result<serde_json::Value, String>; }  // implemented by ramzes-api's SecretKey
pub enum Change { EventRouted{event_id}, DeliveryUpdated{event_ids, delivery_id, state} }
pub struct Engine { /* store, registry, opener, clock, policy, wake: Arc<Notify>, changes: broadcast::Sender<Change>, public_url */ }
impl Engine {
    pub fn new(store, registry, opener, clock, policy: RetryPolicy, public_url: Option<String>) -> Arc<Engine>;
    pub fn wake(&self);                                   // called by ingest after commit
    pub fn subscribe(&self) -> broadcast::Receiver<Change>;
    pub async fn route_once(&self) -> Result<usize>;      // one claim-and-route pass; returns events handled
    pub async fn deliver_once(&self) -> Result<usize>;    // one claim-and-send pass
    pub async fn run(self: Arc<Self>, shutdown: CancellationToken);   // loops both until cancelled; poll every 1 s or on wake
    pub async fn replay_delivery(&self, ws, delivery_id) -> Result<Delivery>;
    pub async fn replay_event(&self, ws, event_id) -> Result<usize>;  // routes again with replay = true
    pub async fn retention_once(&self, ws) -> Result<u64>;
}
```
Delivery pass per delivery: load the connection (missing → `Failed` "connection was deleted"); unknown connector kind → `Failed`; open credentials (error → `Failed`); load the notification's events and build the message; `send`; map `Delivered` → `Finish::Delivered`, `Permanent` → `Finish::Failed`, `Retry` → `next_attempt` → `Finish::Retry` or `Finish::Dead`. Lease is 60 s; up to 8 deliveries run concurrently.

- [ ] Tests (in-memory SQLite store, fake clock, scripted fake connector): an event matching one route with two targets produces two deliveries and both are sent; an unmatched event ends routed with `route_count = 0`; a `Retry` outcome reschedules at the policy's delay and a later pass at the advanced clock delivers; eight `Retry` outcomes end as `dead`; `Permanent` ends `failed` after one attempt; a deleted connection ends `failed` with the reason and the worker continues with the next delivery *(Review Focus 1)*; `replay_delivery` produces a second successful send; `replay_event` creates notifications flagged `replay`; `wake` makes `run` process an event without waiting for the poll; `Change` messages are broadcast; `retention_once` honours the instance setting and a per-topic override.
- [ ] Implement. Commit: `feat(engine): route and delivery workers, replay, retention`.

### Task 12: Ingest

**Files:** `crates/ramzes-ingest/src/lib.rs`

**Produces:**
```rust
pub struct IngestState { pub store: Store, pub engine: Arc<Engine>, pub limits: IngestLimits, pub clock: Arc<dyn Clock> }
pub struct IngestLimits { pub max_body_bytes: usize /* 1 MiB */, pub max_batch: usize /* 1000 */, pub per_source_per_minute: u32 /* 6000 */ }
pub fn router(state: IngestState) -> axum::Router;      // POST /v1/events
pub fn hash_key(key: &str) -> String;                   // sha256 hex, shared with the API
pub fn generate_source_key() -> (String /* "rz_src_…" */, String /* prefix */);
```
Request: `Authorization: Bearer <source key>`; body is one event object or an array; optional `Idempotency-Key` header (≤ 200 chars). Response `202 {"ids": [...], "duplicate": bool}`. Errors are JSON `{"error": "<code>", "message": "<text>"}`: 401 `unauthorized`, 400 `invalid_json`, 422 `invalid_event` (message names the index and the rule), 413 `too_large`, 422 `batch_too_large`, 429 `rate_limited` with `Retry-After`, 503 `unavailable`. Rejected bodies from an authenticated source are stored with `record_source_request`.

- [ ] Tests (router driven with `tower::ServiceExt::oneshot`): a single event and a batch are stored and `wake` is observed; missing or wrong key → 401; invalid JSON → 400; unknown severity, non-string label, empty topic → 422 naming the field; body over the limit → 413; 1001 events → 422 *(Review Focus 3)*; the same `Idempotency-Key` twice → identical ids, `duplicate: true`, one stored event *(Review Focus 5)*; exceeding the per-source rate → 429 with `Retry-After`; a closed database → 503; a rejected body appears in `list_source_requests` with its error.
- [ ] Implement. Commit: `feat(ingest): event API`.

### Task 13: API foundation — crypto and auth

**Files:** `crates/ramzes-api/src/{lib,crypto,auth}.rs`

**Produces:**
```rust
pub struct SecretKey;  impl SecretKey { pub fn from_base64(s: &str) -> Result<Self, String>;   // 32 bytes
    pub fn load_or_create(data_dir: &Path) -> io::Result<Self>;                                 // data_dir/secret.key, mode 0600
    pub fn seal(&self, v: &serde_json::Value) -> String;  pub fn open(&self, sealed: &str) -> Result<serde_json::Value, String>; }
impl ramzes_engine::Opener for SecretKey
pub struct AppState { store, engine, registry, key: Arc<SecretKey>, clock, workspace_id: String, secure_cookies: bool }
pub struct ApiError { status, code, message }          // IntoResponse as {"error","message"}; From<StoreError>
pub struct Auth { pub workspace_id: String, pub user: Option<User> }   // extractor: session cookie or Bearer rz_tok_…
pub fn router(state: AppState) -> (axum::Router, utoipa::openapi::OpenApi);
```
Sealing is XChaCha20-Poly1305 with a random nonce, output `base64(nonce || ciphertext)`. Passwords use Argon2id with a minimum length of 10. Session tokens are 32 random bytes, stored as SHA-256, sent as cookie `ramzes_session` (`HttpOnly`, `SameSite=Lax`, `Path=/`, 30 days, `Secure` when configured). State-changing requests authenticated by cookie require the header `X-Ramzes-Request: 1`.

Endpoints: `GET /api/setup` → `{needs_setup}`; `POST /api/setup {email, password}` (only while no user exists, then logs in); `POST /api/auth/login`; `POST /api/auth/logout`; `GET /api/auth/me`.

- [ ] Tests: seal/open round-trips; a tampered ciphertext and a different key both fail; `load_or_create` creates the file once and reads it back; setup succeeds once, then returns 409; a short password → 422; login with a wrong password → 401 with the same message as an unknown email; `me` without a session → 401; logout invalidates the session; a cookie-authenticated POST without the header → 403; a bearer token authenticates without the header.
- [ ] Implement. Commit: `feat(api): credential sealing and authentication`.

### Task 14: API resources

**Files:** `crates/ramzes-api/src/{sources,connections,routes,events,failures,settings,stream}.rs`

**Produces** (all under `/api`, all require `Auth`, all in the OpenAPI document):

| Method and path | Purpose |
|---|---|
| `GET /sources`, `POST /sources {name}` | List; create (returns the key once) |
| `POST /sources/{id}/rotate`, `DELETE /sources/{id}`, `GET /sources/{id}/requests` | Rotate key; delete; recent rejected requests |
| `GET /connectors` | Descriptors of the built-in connectors |
| `GET /connections`, `POST /connections`, `PUT /connections/{id}`, `DELETE /connections/{id}` | CRUD. Responses never contain credentials, only `has_credentials`. An update without `credentials` keeps the stored ones |
| `POST /connections/{id}/test` | Sends a test message; returns the outcome |
| `GET /routes`, `POST /routes`, `PUT /routes/{id}`, `DELETE /routes/{id}`, `POST /routes/reorder {ids}` | CRUD and order. Rejects an invalid match or an unknown connection id with 422 |
| `GET /events?before&source&topic&min_severity&label&q&since&until&outcome&limit` | Feed page |
| `GET /events/{id}` | Journey |
| `POST /events/{id}/replay` | Re-run through current routes |
| `GET /failures`, `POST /deliveries/{id}/replay`, `POST /failures/replay {delivery_ids}` | Failed and dead deliveries; replay one or many |
| `GET /settings`, `PUT /settings` | `retention_days`, `retention_overrides`, `public_url` |
| `GET /users`, `POST /users`, `DELETE /users/{id}` | Local users |
| `GET /tokens`, `POST /tokens {name}`, `DELETE /tokens/{id}` | Admin API tokens (value returned once) |
| `GET /stream` | Server-sent events: `event` and `delivery` messages from `Engine::subscribe`, plus a comment every 15 s |
| `GET /openapi.json` | The OpenAPI document |

- [ ] Tests (router with in-memory store and the fake connector): every endpoint returns 401 without auth; source creation returns a key that then works on `/v1/events`, and the key is absent from the list; a connection's credentials are never in any response and survive an update that omits them; the connection test reports success and failure from the connector; a route with a bad regex or an unknown connection → 422; reorder changes the list order; the feed honours each query parameter and pages with `before`; the journey for an ingested and delivered event contains route name, notification, delivery and attempt; event and delivery replay create new deliveries; settings validation rejects negative retention; the stream emits a `delivery` message after a delivery pass; the OpenAPI document lists every path in the table.
- [ ] Implement. Commit: `feat(api): admin resources, journey, replay, live stream`.

### Task 15: Server binary

**Files:** `crates/ramzes-server/{build.rs,src/main.rs,src/config.rs,src/serve.rs,src/ui.rs,src/ops.rs,tests/e2e.rs}`

**Produces:** the `ramzes` binary.

- CLI: `ramzes [serve]`, `ramzes backup <file>` (SQLite `VACUUM INTO`; an error on Postgres naming `pg_dump`), `ramzes openapi` (prints the document).
- Config, precedence flag > environment > `--config <file.toml>` > default:

| Setting | Env | Default |
|---|---|---|
| listen | `RAMZES_LISTEN` | `127.0.0.1:7269` |
| data_dir | `RAMZES_DATA_DIR` | `./data` |
| database_url | `RAMZES_DATABASE_URL` | `sqlite://<data_dir>/ramzes.db` |
| secret_key | `RAMZES_SECRET_KEY` | generated into `<data_dir>/secret.key` |
| public_url | `RAMZES_PUBLIC_URL` | none (falls back to the setting) |
| secure_cookies | `RAMZES_SECURE_COOKIES` | `false` |

- Startup: create the data directory, connect, migrate, ensure the workspace, start the engine and an hourly retention task, serve. When no user exists, log `First run: open http://<listen>/setup`.
- `/healthz` (always 200), `/readyz` (200 when the database answers, else 503), `/metrics` (Prometheus text: `ramzes_events_ingested_total`, `ramzes_deliveries_total{connector,outcome}`, `ramzes_delivery_duration_seconds`, `ramzes_queue_unrouted`, `ramzes_queue_pending_deliveries`).
- Embedded UI from `ui/build` with fallback to `200.html` for unknown non-API paths; `build.rs` creates a stub `ui/build/200.html` when the directory is missing so the Rust build never depends on Node.
- Shutdown on SIGINT / SIGTERM: stop accepting, cancel the engine, wait up to 20 s for in-flight deliveries.

- [ ] Tests: config precedence (flag over env over file over default); `backup` produces a readable SQLite file containing an inserted event. End to end, starting the built binary on a random port with a temp data directory and a `wiremock` receiver: setup → create source, connection, route through the API → post an event → the receiver gets one signed request and the journey shows `delivered`; `/readyz` and `/metrics` respond; the binary is killed with SIGKILL while the receiver delays its answer, restarted with a shortened lease (`RAMZES_LEASE_MS`), and the receiver gets the message *(Review Focus 2)*; an unknown path returns the UI fallback and `/api/nope` returns 404 JSON.
- [ ] Implement. Run `make test-pg` and the end-to-end test once with `RAMZES_DATABASE_URL` pointing at Postgres.
- [ ] Commit: `feat(server): binary, config, operations endpoints, embedded UI`.

### Task 16: UI foundation

**Files:** `ui/` scaffold; `ui/src/app.css` (tokens), `ui/src/lib/api/{client.ts,schema.d.ts}`, `ui/src/lib/stores/{session,stream,theme}.ts`, `ui/src/lib/components/{Shell,Nav,SeverityDot,StateBadge,Button,Field,Table,Empty,Toast,Palette}.svelte`, `ui/src/routes/+layout.{ts,svelte}`

- Scaffold with `npx sv create ui` (minimal template, TypeScript), add `@sveltejs/adapter-static` with `fallback: '200.html'`, `export const ssr = false` in the root layout, Tailwind CSS 4, Vitest with Testing Library, ESLint, Playwright.
- `npm run gen:api` runs `cargo run -q -p ramzes-server -- openapi` and feeds it to `openapi-typescript` to write `schema.d.ts`; `client.ts` is a typed `fetch` wrapper that adds `X-Ramzes-Request: 1`, throws a typed error from the `{error, message}` body, and redirects to `/login` on 401.
- Theme tokens as CSS custom properties on `:root` (dark) and `[data-theme="light"]`: background, raised surface, line, text, muted text, accent, the five severities, the five delivery states. Monospace font stack throughout (self-hosted JetBrains Mono via `@fontsource`). The theme store reads the saved choice, else the system preference, and sets `data-theme`.
- `Shell` has a left rail (Feed, Failures, Routes, Connections, Sources, Settings), a live indicator fed by the stream store, and a theme toggle. `Palette` opens on Cmd/Ctrl-K and navigates.
- The Vite dev server proxies `/api` and `/v1` to `127.0.0.1:7269`.

- [ ] Tests (Vitest): the client adds the header and maps an error body to the typed error; the theme store prefers the saved value and toggles `data-theme`; the stream store marks itself live on open and reconnecting on error; `SeverityDot` and `StateBadge` render the token class for each value; `Palette` filters and navigates on Enter.
- [ ] Implement. Run `npm run lint && npm run check && npm test && npm run build`. Commit: `feat(ui): foundation, theme tokens, API client`.

### Task 17: UI — first-run setup and login

**Files:** `ui/src/routes/setup/+page.svelte`, `ui/src/routes/login/+page.svelte`, `ui/src/lib/components/{Steps,CopyBlock}.svelte`

Setup is five steps on one page, each unlocking the next: (1) create the admin; (2) name a source, show its key once; (3) a copyable `curl` for `/v1/events` with the key filled in, and a waiting indicator that turns green when the event arrives on the stream; (4) add a connection, choosing Slack or webhook, with a form built from the connector descriptor and a test button; (5) create a catch-all route to that connection, then replay the test event and show its delivery result with a link to the journey. The root layout redirects to `/setup` while `needs_setup` is true and to `/login` when signed out.

- [ ] Tests: step 2 is disabled until step 1 succeeds; an API error is shown next to the field it concerns; the curl block contains the key and the current origin; the connection form renders one input per descriptor field and masks secret fields; login shows the server's message on 401.
- [ ] Implement. Commit: `feat(ui): first-run setup and login`.

### Task 18: UI — live feed and event journey

**Files:** `ui/src/routes/+page.svelte` (feed), `ui/src/routes/events/[id]/+page.svelte`, `ui/src/lib/components/{FilterBar,EventRow,Timeline,TimelineStep,JsonView,MessagePreview}.svelte`, `ui/src/lib/feed.ts`

- Feed: newest first; filter bar for source, topic, minimum severity, label, text, outcome and time range, all mirrored in the URL; new events from the stream are prepended when they match the filters; a pause control buffers them behind a "N new" button; "load older" pages with `before`; `j`/`k` move the selection and Enter opens it; rows show time, severity, title, source, targets and outcome.
- Journey: header with title, severity, source and time; a timeline with one step for "received", one per matched route, one per delivery with its attempts (time, duration, HTTP status, response excerpt) and its next retry time; "unrouted" explained when nothing matched; raw payload and labels; a message preview; replay buttons for the event and for each delivery. The page updates from the stream.

- [ ] Tests: `feed.ts` serialises filters to and from a query string losslessly, decides whether a streamed event matches the active filters, and buffers while paused; `Timeline` renders a delivered, a retrying, a failed and an unrouted journey with the right step states; replay calls the endpoint and shows the new delivery.
- [ ] Implement. Commit: `feat(ui): live feed and event journey`.

### Task 19: UI — routes, connections, sources, failures, settings

**Files:** `ui/src/routes/{routes,connections,sources,failures,settings}/+page.svelte`, `ui/src/lib/components/{RouteForm,MatchEditor,TargetPicker,ConnectionForm,Confirm}.svelte`

- Routes: ordered list with move up / move down and drag reorder, enable toggle, stop flag, a plain-language summary of each match; form with topic pattern, minimum severity, source, label conditions and targets.
- Connections: list with kind and last result; create and edit from the descriptor; test button; delete with a warning listing the routes that use it.
- Sources: list, create (key shown once), rotate, delete, curl snippet, recent rejected requests with their errors.
- Failures: failed and dead deliveries grouped by error text, with select-all and bulk replay.
- Settings: retention days and per-topic overrides, public URL, users, API tokens (value shown once).

- [ ] Tests: `MatchEditor` produces the exact `RouteMatch` JSON for each condition type and summarises it in words; reorder sends the new id order; the delete warning names the dependent routes; failures group by error and bulk replay sends the selected ids; a newly created token or key is shown once and is absent after reload.
- [ ] Implement. Commit: `feat(ui): routes, connections, sources, failures, settings`.

### Task 20: Browser tests

**Files:** `ui/e2e/{setup.spec.ts,journey.spec.ts,global-setup.ts}`, `ui/playwright.config.ts`

`global-setup` builds the UI and the binary, starts `ramzes` on a free port with a temp data directory, and starts a small HTTP receiver that records requests.

- [ ] `setup.spec.ts`: complete the five steps with a webhook connection pointing at the receiver; assert the receiver got the message and the UI shows the delivery as delivered.
- [ ] `journey.spec.ts`: post an event through the API; it appears in the feed without reload; opening it shows the route and a delivered step; stop the receiver, post again, and the journey shows a retry with the error; the light theme toggle changes `data-theme` and the feed stays readable (text contrast checked on one row).
- [ ] Run `npx playwright test`. Commit: `test(ui): browser tests for setup and journey`.

### Task 21: Packaging and docs

**Files:** `deploy/Dockerfile`, `deploy/compose.yaml`, `deploy/compose.postgres.yaml`, `.github/workflows/release.yml`, `README.md`, `docs/03-architecture.md`, `docs/05-next-steps.md`, `CLAUDE.md`

- `Dockerfile`: Node stage builds `ui/build`; Rust stage builds the release binary; final stage is a slim Debian image running as a non-root user with `RAMZES_LISTEN=0.0.0.0:7269`, a `/data` volume and a health check on `/healthz`.
- `compose.yaml` (SQLite, one service) and `compose.postgres.yaml` (ramzes + Postgres).
- `release.yml`: on a version tag, build the UI once, then binaries for Linux x86-64 and ARM64 and macOS x86-64 and ARM64, and push the container image. This workflow cannot be exercised locally.
- README: what it is, a quickstart (binary and Docker), the ingest example, configuration table, status and roadmap by milestone. Every claim checked against the built binary, with a verification table kept out of the README and reported at hand-off.

- [ ] `docker build` the image, run it, complete setup against it with curl, and deliver one event to a local receiver.
- [ ] `docker compose -f deploy/compose.postgres.yaml up` and repeat the check.
- [ ] Update the docs. Commit: `chore: container image, compose examples, release workflow, README`.

---

## Not in this plan

Milestones 2–4 from the spec. In particular: templates, payload expressions, declarative connectors and adapters, the route tester, config history and export/import, the Helm chart, all shaping features, acknowledgement and escalation. The schema for them exists after Task 5 and stays unused.
