# 05 — Next steps

Milestone 1 (Core) is built: see the README for what works and
`superpowers/plans/2026-10-13-milestone-1-core.md` for how it was built.

1. **Try it** — run it, go through setup, point one real service at it.
2. **Check Slack for real** — the Slack connector is tested against a mock
   server only. Send through a real incoming webhook before relying on it.
3. **Dogfood** milestone 1 in one real project.
4. **Plan milestone 2 (Breadth)** — its own implementation plan, starting with
   the declarative connector and adapter formats.
5. **Milestones 3–4**, each with its own plan, then the public launch.

Still open, in parallel with the build:

- **Verify the landscape** — fresh competitor scan (Novu self-host, Keep,
  Grafana OnCall, apprise-api, anything new in 2026). Update
  `02-landscape.md` before any positioning is published.
- **Validate demand** (Q4) — talk to 5–10 people running microservices: how do
  they send internal notifications today, what hurts?
- **Contributor License Agreement** — set one up before accepting outside
  contributions (see `LICENSING.md`).
