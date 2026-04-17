# ADR-006: State Label Taxonomy

**Status:** Accepted
**Date:** 2026-04-17
**Deciders:** Code Looper project team
**Related:** #190

## Context

The orchestration engine selects its active lifecycle by querying GitHub issue state. For this to work, issue state must be expressible as label-based predicates that the engine can evaluate:

- "Are there issues ready to work?" → look for `ready-for-dev` label
- "Is something blocked?" → look for `blocked` label
- "Is anything actively in progress?" → look for `in-progress` label

The label set must be:
1. **Minimal** — the engine's query logic grows with every new label; fewer is better
2. **Mutually exclusive in intent** — at any point in time, an issue should naturally sit in exactly one state
3. **Gap-free** — every meaningful state in the lifecycle must be representable (even implicitly)

Several label sets were considered during design.

## Decision

Use exactly **three** state labels:

| Label | Meaning |
|---|---|
| `ready-for-dev` | Groomed, scoped, and actionable — safe for the execution lifecycle to pick up |
| `in-progress` | Actively being worked by an agent or person |
| `blocked` | Has an unresolved dependency (another issue, external decision, environment problem, etc.) |

Two additional states are represented **implicitly** (no label):

- **Backlog / needs grooming** — open issue with none of the three labels
- **Done** — closed issue

## Labels Explicitly Excluded

The following labels were considered and rejected:

| Candidate | Reason excluded |
|---|---|
| `selected` | Adds a "selected for sprint" state between backlog and ready-for-dev. Unnecessary — milestone assignment already encodes "selected for this release." A separate `selected` label would duplicate milestone semantics. |
| `ready-for-pr` | Adds a state between in-progress and done. The PR itself is the signal that work is ready for review — a label is redundant with the open PR's existence. The `pr-review` lifecycle queries open PRs directly, not issue labels. |
| `done` | GitHub's "closed" state is the canonical signal for done. A `done` label would duplicate it and risk getting out of sync (closed issues with no `done` label, open issues marked `done`). |
| `needs-review` (issue level) | Code review state lives on the PR, not the issue. The PR-review lifecycle queries PR state directly. |
| `in-review` | Same reasoning as `needs-review` — this is PR state, not issue state. |

## Consequences

**Positive:**
- Three labels means three simple predicates in the orchestration engine's lifecycle selection logic
- Implicit backlog state (no label) allows new issues to land without immediate label assignment — the grooming lifecycle handles them when ready
- Closed = done is a natural GitHub convention that all contributors already understand

**Negative:**
- "Backlog" is not filterable as a label — you must query for "open issues with no state label," which requires a client-side filter step rather than a simple label search
- Label discipline cannot be enforced by GitHub — it relies on the engine and contributors applying labels consistently

## Invariants the Engine Must Maintain

- An issue should have **at most one** state label at a time
- When work starts: remove `ready-for-dev`, apply `in-progress`
- When blocked: remove `in-progress`, apply `blocked`; add a comment explaining the blocker
- When unblocked: remove `blocked`, apply appropriate state label
- When work completes: remove `in-progress`, close the issue (no label needed)

## References

- `docs/project-management.md` — Full label taxonomy and workflow documentation
- ADR-005 — GitHub Issues + milestones as PM primitive (companion ADR)
- #190 — Issue that introduced this system
- #191 — Orchestration engine that consumes these labels
