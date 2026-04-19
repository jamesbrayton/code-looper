# Versioning

code-looper uses [Semantic Versioning](https://semver.org/) (`MAJOR.MINOR.PATCH`). The `version` field in `Cargo.toml` is the single source of truth. The corresponding git tag is always `vX.Y.Z` (with the `v` prefix).

## Bump Rules

| Component | Bump when |
|---|---|
| `PATCH` | Bug fixes, doc updates, dependency bumps, internal refactors with no behavioral change |
| `MINOR` | New features, new CLI flags, new provider adapters — backwards compatible |
| `MAJOR` | Breaking changes to the CLI interface, config schema, or public API |

The project is currently pre-1.0 (`0.x.y`). Minor version bumps may include breaking changes during this phase; this will be noted explicitly in release notes when it occurs.

## How to Cut a Release

1. Ensure the current milestone has zero open issues and zero open PRs
2. Bump `version` in `Cargo.toml` to the new version (e.g., `0.2.0`)
3. Run `cargo build` to update `Cargo.lock`
4. Commit: `git commit -am "chore: bump version to vX.Y.Z (#<issue>)"`
5. Tag: `git tag vX.Y.Z`
6. Push commit and tag: `git push && git push --tags`
7. The `release.yml` workflow fires automatically on the tag push, builds cross-platform binaries, creates the GitHub Release, and closes the milestone
8. Confirm the milestone was closed by the workflow (close it manually only if the automation failed)

## Tag Format

Tags must always use the `v` prefix: `v0.2.0`, `v1.0.0`, etc. Never tag without the prefix (`0.2.0` is not a valid release tag).

## Pre-Release Versions

Use SemVer pre-release suffixes when testing a release candidate before tagging the final version:

- `v0.2.0-beta.1` — early beta, may have known issues
- `v0.2.0-rc.1` — release candidate, feature-complete

The `release.yml` workflow detects pre-release suffixes (any tag matching `v*-*`) and marks the GitHub Release as a pre-release automatically. Pre-release binaries are published the same way as stable releases but are not shown as the "latest" release.
