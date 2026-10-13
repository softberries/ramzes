# Security policy

## Reporting a vulnerability

Please do not report security issues in public issues, pull requests or discussions.

Report them privately through [GitHub's private vulnerability reporting](https://github.com/softberries/ramzes/security/advisories/new), or by email to [hello@softberries.dev](mailto:hello@softberries.dev). Include the commit or version you run, how it is deployed (Docker Compose, Helm or from source), which database it uses, and the steps to reproduce the issue.

You will get a reply acknowledging the report. Once the issue is confirmed, a fix is prepared in a private advisory and released, and the advisory is published with credit to you unless you ask otherwise.

## Supported versions

ramzes has no release yet. Until it has, security fixes go into the `main` branch. After the first release they will go into the latest release.

## What to know before you deploy

The [Security](https://softberries.github.io/ramzes/operations/security/) page of the documentation describes how sign-in, API tokens, source keys and stored credentials work, and what ramzes does not have: roles, single sign-on and an audit log. Everyone who can sign in is an administrator.
