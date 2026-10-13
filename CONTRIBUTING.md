# Contributing to ramzes

Bug reports, questions and feature requests are welcome as [issues](https://github.com/softberries/ramzes/issues). Before a larger change, open an issue to discuss it, so that nobody spends time on something that will not be merged.

## Pull requests and the Contributor License Agreement

ramzes is offered under the AGPLv3, and a commercial edition is planned (see [LICENSING.md](LICENSING.md)). To make that possible, every contributor will sign a Contributor License Agreement before a pull request can be merged.

**The agreement is not set up yet, so outside pull requests are not being merged yet.** You are welcome to open one to show a fix or an idea; it will be picked up once the agreement is in place. This page will say so when it is.

Questions: [hello@softberries.dev](mailto:hello@softberries.dev).

## Building and testing

The tools you need and every developer command are in [From source](https://softberries.github.io/ramzes/install/from-source/): Rust 1.98, Node 24, and Docker for the tests that use Postgres.

| What | Command |
|---|---|
| Rust tests on SQLite | `make test` |
| The same tests on SQLite and on a throwaway Postgres in Docker | `make test-pg` |
| Format and clippy with warnings as errors | `make lint` |
| Web UI lint, type check, unit and component tests | `make ui-test` |
| Browser tests against the real binary | `make e2e` |
| Documentation site | `npm ci && npm run build` in `site/` |

## Before you open a pull request

CI runs the commands above on every pull request; run them locally first.

- Keep a pull request to one change, and explain what it does and why.
- Add or update tests for behaviour you change. A bug fix comes with a test that failed before it.
- Update the documentation in `site/` when you change something users see.
- A channel that is one HTTP request is a YAML definition in `crates/ramzes-connectors/src/definitions/`, not Rust code. A third-party sender is a YAML mapping in `crates/ramzes-core/src/adapters/`.
- Storage: one set of migrations for SQLite and Postgres, only `TEXT` and `BIGINT` columns, SQL written once with `$N` placeholders (see `crates/ramzes-store/src/lib.rs`).
- UI colours come only from the tokens in `ui/src/app.css`.
