# Project Management Conventions

This document describes the label system, milestone conventions, and lifecycle model used to manage work in this repository. These conventions give the orchestration engine (see [#191](https://github.com/jamesbrayton/code-looper/issues/191)) queryable state signals to drive lifecycle decisions without requiring JIRA or GitHub Projects.

## Label System

### State Labels

State labels reflect where an issue is in the development lifecycle. They drive orchestration queries — the engine checks for these labels to select the active lifecycle.

| Label | Color | Meaning |
|---|---|---|
| `ready-for-dev` | green | Groomed, scoped, and actionable — safe to pick up for execution |
| `in-progress` | blue | Actively being worked by an agent or person |
| `blocked` | red | Has an unresolved dependency (another issue, external decision, etc.) |

**Implicit states (no label required):**

- **Backlog / needs grooming** — open issue with none of the state labels above
- **Done** — closed issue

An issue without a state label is in the backlog and may or may not be ready for work. The absence of a state label is a signal to the grooming lifecycle that the issue needs attention before execution.

### Priority Labels

Priority labels indicate importance relative to the current milestone. They are assigned during grooming.

| Label | Color | Meaning |
|---|---|---|
| `priority-high` | red-orange | Must be in the current milestone; blocks release if incomplete |
| `priority-medium` | yellow | Should be in the current milestone; deferrable if needed |
| `priority-low` | light blue | Nice to have; default candidate for deferral to next milestone |

### Type Labels (Retain As-Is)

These labels describe issue *type*, not lifecycle state. They complement the above and are not replaced by them.

| Label | Meaning |
|---|---|
| `bug` | Something isn't working correctly |
| `enhancement` | New feature or improvement |
| `tech-debt` | Internal quality or maintainability improvement |
| `discovered-during-loop` | Surfaced by an automated loop run |

## Milestone Conventions

Each milestone corresponds to one release. Milestones are the primary scope boundary for execution.

- A milestone is **complete** when it has zero open issues and zero open PRs
- Milestone completion triggers the release lifecycle: tag push → `release.yml` → GitHub Release → milestone closed
- Issues **not** assigned to any milestone are **backlog** — visible to grooming, invisible to execution
- Issues assigned to a **future** milestone (e.g., `v0.3.0`) are **deferred** — explicitly out of scope for the current release

### Release Sequence

1. All issues in the current milestone are closed; all PRs merged
2. Release lifecycle fires: bump `Cargo.toml` version, commit, push `vX.Y.Z` tag
3. `release.yml` triggers on the tag — builds cross-platform binaries, creates GitHub Release with auto-generated notes
4. Release lifecycle closes the milestone

See `docs/versioning.md` (issue [#186](https://github.com/jamesbrayton/code-looper/issues/186)) for the full versioning and release process.

## Orchestration Lifecycles

The engine selects the active lifecycle by querying GitHub state at the start of each iteration:

| Condition | Lifecycle |
|---|---|
| Current milestone has open `ready-for-dev` issues | `execution` |
| Open PRs on milestone issues, no `ready-for-dev` issues | `pr-review` |
| Milestone fully closed (zero open issues + PRs) | `release` |
| Ungroomed open issues exist (no state label) | `grooming` |
| `ready-for-dev` issues exist with no milestone | `planning` |

See [#191](https://github.com/jamesbrayton/code-looper/issues/191) for full orchestration design and ADRs.

## Workflow for New Issues

1. Issue is created — no state label (enters backlog implicitly)
2. **Grooming lifecycle** reviews the issue: clarifies scope, writes acceptance criteria, adds type/priority labels
3. **Planning lifecycle** assigns the issue to a milestone
4. Issue receives `ready-for-dev` label — now visible to the execution lifecycle
5. **Execution lifecycle** picks up the issue, sets `in-progress`
6. Work is done and PR is merged — issue is closed (enters Done implicitly)

## Applying Labels

When updating issue state:

- Set `in-progress` when an agent or person begins active work on the issue
- Remove `in-progress`, close the issue when work is committed
- Set `blocked` if a dependency arises mid-work; add a comment explaining the blocker
- Remove `blocked` and re-set to the appropriate state when the dependency resolves

Only one state label should be active at a time.
