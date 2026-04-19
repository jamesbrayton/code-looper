# ADR-007: Tag-Based Release Trigger

**Status:** Accepted
**Date:** 2026-04-17
**Deciders:** Code Looper project team
**Related:** #185, #188

## Context

The release pipeline needs a trigger condition — the event that causes `release.yml` to fire and binaries to be built and published. The candidates were:

1. **Every merge to `main`** — release on every mainline commit
2. **Automated version bump** — a bot or workflow increments the version and commits a tagged release automatically
3. **Manual `vX.Y.Z` tag push** — a human (or orchestration lifecycle) pushes a tag to signal intent to release

## Decision

Releases are triggered by manually pushing a `vX.Y.Z` git tag.

```yaml
on:
  push:
    tags:
      - 'v[0-9]*.[0-9]*.[0-9]*'
```

GitHub Actions tag filters use glob syntax, not regex — `*` matches any sequence of characters. The pattern above matches `v0.2.0`, `v1.0.0`, and also pre-release tags like `v0.2.0-beta.1`. The `release.yml` workflow detects hyphens in the tag name and marks those releases as pre-releases on GitHub.

The full release sequence (per #185 and `docs/versioning.md`):
1. Milestone reaches zero open issues and zero open PRs
2. Bump `version` in `Cargo.toml`, commit, push `vX.Y.Z` tag
3. `release.yml` fires on the tag push
4. Workflow builds binaries, creates GitHub Release, closes milestone

## Consequences

**Positive:**
- Release is an explicit, intentional act — no accidental releases from routine commits
- Tag serves as a durable, named anchor point in git history for every release
- Easy to automate: the orchestration release lifecycle pushes the tag when it determines the milestone is complete
- Pre-release support is natural: append a suffix to the tag name

**Negative:**
- Slightly more process than push-to-main; the release author must remember to bump `Cargo.toml` and push both the commit and the tag
- No automated version bump — risk of forgetting to increment the version before tagging. Mitigated by `docs/versioning.md` documenting the exact steps.

## Alternatives Rejected

- **Every merge to `main`**: Appropriate for continuous delivery products with fast feedback cycles. This project's release unit is a milestone, not a commit. Shipping on every merge would produce dozens of releases per milestone with no meaningful versioning signal.
- **Automated version bump bot**: Adds a dependency on a GitHub App or custom action. Adds complexity without clear benefit at this project's scale. The orchestration release lifecycle can push a tag without needing a bot.

## References

- `.github/workflows/release.yml` — the workflow triggered by this pattern
- `docs/versioning.md` — step-by-step release process
- ADR-008 — milestone as release boundary (companion ADR)
- #185 — CI/CD maturity umbrella issue
