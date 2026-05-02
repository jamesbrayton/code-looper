# ADR-011: Depth vs Breadth Iteration Patterns

**Status:** Accepted
**Date:** 2026-05-02
**Deciders:** Code Looper project team
**Related:** #191

## Context

After a release fires (milestone complete), the engine must decide what to do next. Two plausible behaviors emerged:

1. **Stop** — the milestone is done; the run ends after the release lifecycle.
2. **Continue to the next milestone** — plan new work, start execution again.

These behaviors suit different workflows and user risk tolerances.

## Decision

Two iteration patterns:

| Pattern | Post-release behavior | Best for |
|---------|-----------------------|---------|
| `depth` | Stop after release; milestone scope is locked to the launch prompt goal | Quality-first, deliberate release cycles |
| `breadth` | After release, enter planning lifecycle to set up the next milestone; repeat | High-velocity, MVP phases with frequent user feedback |

`depth` is the **default** because:
- It matches the mental model of most users who launch a loop for a specific goal.
- It is safer: the user retains control over what comes next after a release.
- It is easier to reason about: the run has a defined end state.

`breadth` is appropriate when the user trusts the agent's planning judgment and wants fully autonomous velocity. It requires `autonomous` mode.

## Consequences

- In `depth` mode, the engine stops when the `release` lifecycle fires (or raises an error if no more work exists in the milestone).
- In `breadth` mode, the engine transitions from `release` → `planning` automatically. This requires `autonomous` mode; attempting `breadth` with `execution-only` or `assisted` is a startup validation error.
- `breadth` cross-milestone behavior is implemented as a follow-on iteration: after release, the next `select()` call naturally falls through to `planning` because the old milestone is closed and a new one does not yet exist.
