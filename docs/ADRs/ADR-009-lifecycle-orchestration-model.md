# ADR-009: Lifecycle Orchestration Model

**Status:** Accepted
**Date:** 2026-05-02
**Deciders:** Code Looper project team
**Related:** #191

## Context

The existing policy engine selects one of three workflow branches (pr-review, issue-execution, backlog-discovery) using user-configurable condition rules evaluated against total open PR and issue counts. This is flexible but not milestone-aware: the engine cannot distinguish between "issues ready for the agent to work" and "issues that need grooming first", and it has no concept of a release trigger.

The milestone-as-release-boundary pattern (ADR-008) and the state label taxonomy (ADR-006) established that GitHub milestone + `ready-for-dev` label is the queryable signal for "work that is ready for the agent." A richer lifecycle model should exploit these signals.

## Decision

Define **five named lifecycles** selected by querying GitHub state at the start of each iteration:

| Lifecycle | Selected when |
|-----------|--------------|
| `execution` | Current milestone has ≥1 open issue with `ready-for-dev` label |
| `pr-review` | Open PRs exist and no `ready-for-dev` issues in milestone |
| `release` | Milestone has 0 open issues and 0 open PRs |
| `grooming` | Open issues in milestone have no state label (ungroomed backlog) |
| `planning` | `ready-for-dev` issues exist with no milestone assignment |

Selection is evaluated in priority order: execution → pr-review → release → grooming → planning. The first matching condition wins.

The existing three-branch policy engine is retained for backward compatibility when `orchestration.mode` is not set. When `mode` is set, the new `LifecycleEngine` replaces it.

## Consequences

- More granular repository context is required: label-filtered issue counts per milestone, not just total counts.
- The `gh` CLI is called with `--milestone` to fetch milestone issues; label filtering is applied in-process. A third `gh api` call fetches backlog issues with no milestone assignment. This adds one extra shell invocation per iteration compared to the legacy resolver (3 total vs 2).
- The `release` lifecycle fires as a soft signal: the orchestration engine selects it and generates a prompt instructing the agent to create and push a release tag, which in turn triggers `release.yml`. No direct CI invocation from the engine.
- Grooming and planning lifecycles are restricted by autonomy mode (see ADR-010).
