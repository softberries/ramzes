# Milestone 2 (Breadth) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Tests first.

**Goal:** Add expressions, templates, declarative connectors and source adapters, the remaining chat / email / push channels, the route tester, config history with export/import, and a Helm chart.

**Architecture:** MiniJinja is the one language for route expressions, templates, connector request bodies and inbound mappings; it lives in `ramzes-core::expr` with a fuel limit. Connectors and source adapters are YAML definitions compiled into the binary, plus user-added ones stored in the database. A connector can render the request it would send without sending it, which serves the template editor and the journey preview.

**Tech Stack:** as milestone 1, plus `minijinja` 3 (features `json`, `fuel`), `serde_norway` (YAML), `lettre` (SMTP).

**Spec:** `docs/superpowers/specs/2026-10-13-ramzes-design.md` (§5 matching, §6 connectors, §7 screens marked 2, §8 export/import, §12).

## Global Constraints

- Everything in the milestone 1 plan's Global Constraints still holds (one migration set, `TEXT`/`BIGINT` only, `$N` SQL, no `allow(dead_code)`, tests first, colours from tokens).
- Schema changes go in `0002_breadth.sql`; `0001_init.sql` is never edited.
- Expression and template evaluation is bounded (fuel) and never panics on user input.
- Secrets never leave the server: previews, exports and revisions shown by the API carry no credentials.
- Each connector's request format is checked against the service's current documentation before its definition is written; what could not be checked is listed in the README table.

## Review Focus

1. A template or expression that loops forever or is huge: evaluation stops with an error, the worker keeps running.
2. Event text containing quotes, newlines or `{{`: the rendered JSON body stays valid and the text is not evaluated as a template.
3. A third-party payload with oversized or oddly named fields: the adapter clamps or cleans them instead of rejecting the request.
4. A forged or missing signature on a per-source URL: 401, nothing stored as an event.
5. Importing a config that names connections whose credentials are absent: existing credentials are kept, nothing is silently wiped.

---

### Task 1: Expressions and templates in core

**Files:** `crates/ramzes-core/src/expr.rs`, `matching.rs`, `routing.rs`, `message.rs`; tests `crates/ramzes-core/tests/expr.rs`, `routing.rs`.

**Produces:**
- `expr::check_expression(&str) -> Result<(), String>`, `expr::check_template(&str) -> Result<(), String>`
- `expr::eval(&str, &serde_json::Value) -> Result<serde_json::Value, String>`, `expr::eval_bool`, `expr::render(&str, &Value) -> Result<String, String>`
- `event_context(&Event) -> Value` (`topic, title, body, severity, status, labels, payload, source_id, id`)
- `RouteMatch.expr: Option<String>`; `RouteMatch::evaluate(&Event) -> Result<bool, String>`; `matches` stays and treats an error as no match
- `Target.template_id: Option<String>`
- `Template { id, name, title, body }`, `apply_template(&Template, &Message, &[Event]) -> Result<Message, String>`
- `route_event` returns `Routed { matched: Vec<Matched>, errors: Vec<(route_id, String)> }`

**Tests:** comparison and boolean expressions over payload and labels; missing attribute is false, not an error; syntax error reported by `check_expression`; fuel stops `{% for %}` over a huge range; `render` does not re-evaluate text that contains `{{`; a route whose expression errors does not match and the error is returned; template renders title and body from event fields; template with a syntax error is rejected by `check_template`.

### Task 2: Schema 0002 and store additions

**Files:** `crates/ramzes-store/migrations/0002_breadth.sql`, `src/{templates,definitions,revisions}.rs`, changes to `routes.rs`, `sources.rs`, `deliveries.rs`, `connections.rs`; contract tests.

**Schema:** `routes.last_error TEXT, last_error_at BIGINT`; `deliveries.warning TEXT`; `sources.key_sealed TEXT`; `source_requests.headers TEXT`; `templates` gets `title TEXT NOT NULL DEFAULT ''`; `connector_definitions.kind TEXT` (unique per workspace).

**Produces:** template CRUD; connector definition CRUD; `set_route_error`; `StoredRoute { route, last_error, last_error_at }` from `list_routes_with_errors`; source `mapping`, `key_sealed` read/write, `get_source`; `record_source_request(.., headers)`; `add_revision`, `list_revisions`, `get_revision`; `connection_health(workspace, since) -> per connection delivered/failed/last_error`; delivery `warning` via `Finish`.

**Tests (both databases):** each CRUD round-trips; duplicate definition kind conflicts; revisions list newest first and trims to 200; health counts by state; a route error is stored and cleared.

### Task 3: Declarative connectors and preview

**Files:** `crates/ramzes-connectors/src/{declarative,definitions/*.yaml,lib,http,slack,webhook}.rs`; tests with wiremock and snapshots.

**Produces:**
- `ConnectorDescriptor` with owned strings and `source: builtin | bundled | custom`
- `Connector::preview(&ConnectionConfig, &Message, Option<&str>, now) -> Result<Preview, String>`; `Preview { method, url, headers, body }` with secrets masked in `url` and `headers`
- `Definition` (YAML): `kind, name, description, destination_label, fields, request { method, url, headers, body, format }, success, retry_after_header, max_len`; `Definition::parse(&str) -> Result<Definition, String>` validates templates
- `Registry::builtin(HttpOptions)`, `Registry::set_custom(Vec<Definition>)`, `Registry::get`
- Bundled: Discord, Microsoft Teams (Workflows webhook, Adaptive Card), ntfy, Telegram, Pushover
- `HttpOptions { timeout, block_private: bool }`: with `block_private`, requests to loopback, private, link-local and unique-local addresses are refused (also after DNS resolution)

**Tests:** snapshot of the exact request per bundled definition; quotes and newlines in the title keep the JSON valid; `retry_after_header` honoured; a non-success status outside `success` classified by `classify`; invalid YAML and an invalid template are rejected with the field named; preview masks secret values; private address refused when blocking is on and allowed when off.

### Task 4: Email connector

**Files:** `crates/ramzes-connectors/src/email.rs`; test with an in-process SMTP listener.

**Produces:** kind `email`; settings `host, port, security (starttls|tls|none), username, from`; credential `password`; destination = comma-separated recipients (falls back to setting `to`). Plain-text and HTML parts.

**Tests:** the listener receives MAIL FROM, RCPT TO for every recipient and a body with the title as subject; a refused recipient is a permanent failure; an unreachable server is a retry; header injection through the title (CR/LF) is not possible.

### Task 5: Engine uses templates, expressions and previews

**Files:** `crates/ramzes-engine/src/{route_worker,delivery_worker,render}.rs`; tests.

**Produces:** `Engine::render(workspace, connection, template_id, events) -> Rendered { message, warning }`; `Engine::preview_delivery`; route errors written with `set_route_error`; a template that fails to render sends the default message and stores a warning on the delivery; per-connection concurrency limit of 4.

**Tests:** route with expression routes only matching events; expression error recorded on the route and cleared after a clean pass; template applied to the sent message; broken template sends the default and sets the warning; no more than 4 simultaneous sends per connection.

### Task 6: Source adapters

**Files:** `crates/ramzes-core/src/mapping.rs`, `crates/ramzes-ingest/src/{lib,adapters,definitions/*.yaml}.rs`; tests with real sample payloads.

**Produces:**
- `Mapping` (YAML): `verify { method: bearer | header | hmac_sha256 | query, header, prefix, param }`, `events` (expression giving a list, optional), `when` (optional), `fields { topic, title, severity, status, body, labels, dedup_key, ts }`
- `Mapping::apply(payload, headers) -> Result<Vec<NewEvent>, String>` — clamps lengths, cleans topic and label keys, maps unknown severities to `info`
- `POST /v1/in/{source_id}`; bundled kinds `alertmanager, grafana, github, gitlab, sentry`; kind `json` with a mapping stored on the source
- Every request to an adapter source is kept (newest 20) with its headers minus secrets

**Tests:** each bundled adapter maps a documented sample payload (snapshot of events); Alertmanager batch gives one event per alert with `resolved` status mapped; GitHub HMAC accepted, forged rejected; GitLab token header; query-key; `when` false answers 202 with no events; oversized title clamped; mapping error answers 422 and is recorded.

### Task 7: Admin API

**Files:** `crates/ramzes-api/src/{templates,definitions,tester,config,sources,connections,routes,events}.rs`; API tests.

**Produces:** `/api/templates` CRUD and `POST /api/templates/preview`; `/api/connector-definitions` CRUD (registry refreshed); `/api/source-kinds`; source create with `kind` and `mapping`, `PUT /api/sources/{id}/mapping`, `POST /api/sources/preview-mapping`; `POST /api/routes/test`; `GET /api/events/{id}/preview`; connection health in `GET /api/connections`; `/api/config/revisions`, `.../{id}`, `.../{id}/rollback`, `GET /api/config/export`, `POST /api/config/import`; a revision is recorded after every config change.

**Tests:** each endpoint's happy path and its main rejection; tester with unsaved routes writes nothing; export contains no credentials; import keeps stored credentials; rollback restores routes and records a new revision.

### Task 8: CLI export / import, address policy flag

**Files:** `crates/ramzes-server/src/{main,config,serve}.rs`; e2e tests.

**Produces:** `ramzes export [file]`, `ramzes import <file>`; `--block-private-addresses` / `RAMZES_BLOCK_PRIVATE_ADDRESSES`; custom definitions loaded into the registry at start.

### Task 9: UI

Templates page with preview; connector catalogue and health on Connections; custom connector editor; source kinds, per-source URL, recent requests and mapping editor with live preview; route editor gains expression and template per target, and the tester panel; journey shows the rendered message; feed shows targets; Settings gains config history with diff and rollback, export and import. Component tests for new logic, browser tests for the route tester and a template preview.

### Task 10: Helm chart, docs, final review

`deploy/helm/ramzes` (StatefulSet with a volume for SQLite, or external Postgres via values; Service; optional Ingress), checked with `helm lint` and `helm template`. README and docs updated with a verification table. Whole-branch review, one fix pass.
