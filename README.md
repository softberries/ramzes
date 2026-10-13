# ramzes

> Lightweight, self-hosted notification router for microservices.
> "Alertmanager for any event."

**Status:** design written, awaiting review; no code yet (started 2026-10-13)

## One-liner

Services and tools fire events at ramzes over HTTP. ramzes persists
them, applies routing rules (match, dedup, group, rate-limit, quiet hours,
escalate) and delivers them to Slack, MS Teams, email, Discord, webhooks, ntfy,
PagerDuty… A web UI lets you browse every message, see delivery status, replay,
and edit routes without redeploying anything.

## Documents

| File | What's in it |
|---|---|
| [docs/01-idea.md](docs/01-idea.md) | Problem, pitch, target users |
| [docs/02-landscape.md](docs/02-landscape.md) | Existing tools and where the gap is |
| [docs/03-architecture.md](docs/03-architecture.md) | Architecture summary and milestones |
| [docs/04-open-questions.md](docs/04-open-questions.md) | Scoping questions and the answers given |
| [docs/05-next-steps.md](docs/05-next-steps.md) | What happens next |
| [docs/superpowers/specs/2026-10-13-ramzes-design.md](docs/superpowers/specs/2026-10-13-ramzes-design.md) | **The full design (source of truth)** |
