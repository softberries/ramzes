# 05 — Next steps

All four milestones of the design are built: see the README for what exists and
`superpowers/plans/` for how each was built.

Before a public launch:

1. **Run every connector against its live service.** None has been. Formats
   follow the documentation as read on 13 October 2026 and are pinned by
   snapshot tests, which proves they have not changed, not that the services
   accept them. Order by likely use: Slack (webhook and app), email through a
   real provider with TLS, Teams, Discord, PagerDuty, Telegram, ntfy, Pushover.
2. **Receive from every live source**: Alertmanager, Grafana, GitHub, GitLab,
   Sentry.
3. **Dogfood** it in one real project for a few weeks.
4. **Run the release workflow** once; it has never run.
5. **Install the Helm chart** on a cluster; it has only been linted.
6. **Go through the UI by hand** on a phone and in the light theme.

Known gaps in what is built:

- Templates and expressions have no memory limit (see the README).
- `POST /v1/in/{source_id}` answers 404 for an unknown source before it checks
  anything, so it tells a caller whether an id exists. Ids are random.
- A notification's page shows the escalation chain its route has now, not the
  one it had when the notification was sent.
- The Notifications, Silences and Overview pages have browser tests but no
  component tests of the whole page.

Still open:

- **Verify the landscape** — fresh competitor scan (Novu self-host, Keep,
  Grafana OnCall, apprise-api, anything new in 2026). Update
  `02-landscape.md` before any positioning is published.
- **Validate demand** (Q4) — talk to 5–10 people running microservices: how do
  they send internal notifications today, what hurts?
- **Contributor License Agreement** — set one up before accepting outside
  contributions (see `LICENSING.md`).
- **Opsgenie** shuts down on 5 April 2027; decide whether the connector ships.
