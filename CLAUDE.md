# ramzes — context for Claude

Lightweight, self-hosted notification router for microservices
("Alertmanager for any event"). Single Rust binary. Router only: no pub/sub,
no NATS protocol.

**Status:** design written, awaiting Kris's review. No code yet.

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
- Don't start coding until the design is approved and the milestone's implementation plan is written.
