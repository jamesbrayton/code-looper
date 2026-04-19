# ADR-008: Milestone as Release Boundary

**Status:** Accepted
**Date:** 2026-04-17
**Deciders:** Code Looper project team
**Related:** #185, #190, #191

## Context

Code Looper needs a clear answer to: "what gets shipped in a release, and when is a release ready?" Without a defined release boundary, releases can be vague ("everything since last time") or premature (shipping with known open issues).

The options considered were:

1. **Time-based releases** — ship on a cadence (weekly, monthly) regardless of what's done
2. **Feature-flag gating** — ship when specific features are enabled, regardless of issue state
3. **Milestone completion** — ship when a defined set of issues is closed and all PRs merged

## Decision

A GitHub milestone defines both the **scope** (what gets worked) and the **release unit** (what gets shipped). A release is ready when its milestone has zero open issues and zero open PRs.

The milestone lifecycle:
- **Open milestone with open issues** → execution and PR-review lifecycles are active
- **Open milestone with zero open issues/PRs** → release lifecycle fires
- **Closed milestone** → release is complete; this is the final state

The release workflow (`release.yml`) automatically closes the milestone as its last step, after the GitHub Release is created. This closure is the canonical signal that the release is complete.

## Consequences

**Positive:**
- Release scope is explicit and visible before work begins — anyone can see what's in a milestone
- "Ready to release" has a mechanical definition: `milestone.open_issues == 0 && open_prs_on_milestone == 0`
- The orchestration engine can query this condition without ambiguity
- Milestone closure is a permanent, auditable event in GitHub's history
- Discovered work during execution (bugs, regressions) blocks the release by default — quality gate is enforced automatically

**Negative:**
- Milestone scope must be set deliberately before execution begins; an empty milestone immediately triggers the release lifecycle
- Discovery of significant new work mid-milestone forces a scope decision: add to current milestone (blocks release) or defer to next milestone. This is intentional but requires judgment.
- If a tag is pushed before the milestone is clean, the milestone closes automatically regardless of remaining issues. The release sequence in `docs/versioning.md` documents that checking milestone state is the first step.

## Closing Milestone as Final Release Step

The decision to close the milestone *after* the GitHub Release is created (not before) is deliberate:

- The GitHub Release URL is the artifact the world sees; it should exist before the milestone is declared complete
- Closing the milestone before publishing the release would make the milestone appear done while the actual deliverable is still in flight
- The `release.yml` workflow handles this sequencing automatically

## Alternatives Rejected

- **Time-based releases**: Works well for products with many users who need predictable update cycles. Code Looper's release cadence is milestone-driven; shipping half-finished milestones on a calendar schedule would reduce release quality without meaningful user benefit.
- **Feature-flag gating**: Adds runtime complexity for no benefit at this scale. The milestone is a simpler, visible mechanism that maps directly to the project's planning model.

## References

- `docs/project-management.md` — milestone conventions and the lifecycle query model
- `docs/versioning.md` — step-by-step release process that starts with milestone verification
- `.github/workflows/release.yml` — the workflow that closes the milestone after publishing the release
- ADR-005 — GitHub Issues + milestones as PM primitive
- ADR-007 — tag-based release trigger (companion ADR)
- #185 — CI/CD maturity umbrella issue
- #191 — orchestration engine that uses milestone state as a lifecycle signal
