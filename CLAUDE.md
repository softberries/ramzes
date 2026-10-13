# ramzes — context for Claude

Lightweight, self-hosted notification router for microservices
("Alertmanager for any event"). Single Rust binary. Router only: no pub/sub,
no NATS protocol.

**Status:** all four milestones of the design are built and tested. Nothing is released; no connector has been run against its live service.

## Read first
- `docs/superpowers/specs/2026-10-13-ramzes-design.md` — the agreed design, source of truth
- `README.md` — overview and doc index
- `docs/01-idea.md` — problem, pitch, positioning (internal/ops notifications, not Novu-style product notifications)
- `docs/02-landscape.md` — competitors (unverified, written from memory)
- `docs/03-architecture.md` — architecture summary + milestones
- `docs/04-open-questions.md` — scoping questions with answers (Q4 demand validation still open)
- `docs/05-next-steps.md` — what happens next

## Working agreements
- Update the docs when decisions are made (answers → architecture/scope).
- Stack: Rust (Axum, Tokio), SQLite by default or Postgres, MiniJinja, SvelteKit UI embedded in the binary.
- Licence: core AGPL-3.0-only, commercial enterprise edition, CLA (same model as Tayga).
- Each milestone gets its own implementation plan under `docs/superpowers/plans/` before it is coded.
- Shaping decisions (`ramzes-core::shaping`) are pure; the engine applies them under one in-process lock, because ramzes is single-node.
- A channel that is one HTTP request is a YAML definition in `crates/ramzes-connectors/src/definitions/`, not Rust code; a third-party sender is a YAML mapping in `crates/ramzes-core/src/adapters/`.
- Storage rules: one migration set for SQLite and Postgres, only `TEXT` and `BIGINT` columns, SQL written once with `$N` placeholders (see `crates/ramzes-store/src/lib.rs`).
- Pure logic (matching, routing, backoff) lives in `ramzes-core` and takes the time as an argument.
- UI colours come only from the tokens in `ui/src/app.css`.

## Commands
- `make test` — Rust tests on SQLite; `make test-pg` — also on a throwaway Postgres (Docker)
- `make lint` — rustfmt + clippy with warnings as errors
- `make ui-test` — UI lint, type check, unit tests; `make e2e` — browser tests against the real binary
- `npm --prefix ui run gen:api` — regenerate UI types after changing the admin API
