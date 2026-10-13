# Releasing ramzes

The release checklist. No release has been made yet, and the release workflow has never run: treat the first run as a test of the workflow itself, and expect to fix it.

## Before you start

- The workspace version in `Cargo.toml` matches the tag you are about to push, and so do `version` and `appVersion` in `deploy/helm/ramzes/Chart.yaml`. The release workflow checks the first and stops if it differs.
- CI is green on the commit you will tag.
- `site/src/content/docs/changelog.mdx` describes the release.

## 1. Tag the release

```sh
git checkout main && git pull
git tag -a v0.1.0 -m "v0.1.0"
git push origin v0.1.0
```

The tag starts `.github/workflows/release.yml`, which:

- checks the tag against the version in `Cargo.toml`;
- builds the web UI once, then the binary for Linux (x86_64, aarch64) and macOS (aarch64, x86_64), each packed as `ramzes-v0.1.0-<target>.tar.gz` with a `.sha256` beside it;
- builds the image for `linux/amd64` and `linux/arm64` and pushes the manifest list to `ghcr.io/softberries/ramzes`;
- creates the GitHub release with the archives and checksums.

Watch the run in the Actions tab until every job is green.

## 2. Verify the release assets

```sh
gh release view v0.1.0 --json assets --jq '.assets[].name'
gh release download v0.1.0 --dir /tmp/ramzes-release
(cd /tmp/ramzes-release && for f in *.sha256; do shasum -a 256 -c "$f"; done)
docker buildx imagetools inspect ghcr.io/softberries/ramzes:0.1.0   # lists linux/amd64 and linux/arm64
```

Unpack the archive for your machine, start the binary on an empty data directory and go through the setup screen.

## 3. Make the GHCR package public

New GHCR packages are private. In GitHub, open the organisation's **Packages**, then for `ramzes`: **Package settings → Danger Zone → Change visibility → Public**. Check from a machine with no GHCR login:

```sh
docker logout ghcr.io
docker pull ghcr.io/softberries/ramzes:0.1.0
```

## 4. Update what the documentation says about installing

Until the first release the documentation tells people to build from a checkout. After it, update `site/src/content/docs/install/*.mdx`, the quickstart and the README to use the published image and archives, and `deploy/compose*.yaml` if they should pull instead of build.

## 5. Smoke-test on a clean machine

On a machine with no checkout and no GHCR login, follow the quickstart as the documentation now gives it, to the point where a message arrives in a channel.

## 6. Announce

Only after steps 1 to 5 pass, and after the first items of the "Verified claims" page are done: every connector sent through its live service, every source adapter received from its live tool.
