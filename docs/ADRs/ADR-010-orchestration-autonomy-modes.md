# ADR-010: Orchestration Autonomy Modes

**Status:** Accepted
**Date:** 2026-05-02
**Deciders:** Code Looper project team
**Related:** #191

## Context

The five-lifecycle model (ADR-009) gives the engine the ability to perform grooming (scoping issues) and planning (creating new milestones) in addition to execution and PR review. Not all users want the engine to operate at that autonomy level — some want to retain human control over scope decisions.

## Decision

Three autonomy modes are defined at launch time via `orchestration.mode`:

| Mode | Available lifecycles | Human responsibility |
|------|---------------------|---------------------|
| `execution-only` | execution, pr-review | Issue grooming, milestone assignment, release tagging |
| `assisted` | execution, pr-review, grooming | Milestone assignment, release tagging |
| `autonomous` | all five | Launch config, feedback review |

Mode is a **launch-time configuration**, not a runtime toggle. Changing mode mid-run would create unpredictable behavior: an autonomous run might create issues that an execution-only run then refuses to groom.

`execution-only` is the **default** when `mode` is set, so users who forget to specify a mode get the safest option.

## Consequences

- `LifecycleEngine::select()` checks the configured mode before returning a lifecycle; an unavailable lifecycle causes a clear error rather than silent fallback.
- Users running in `execution-only` mode must set up issues with `ready-for-dev` labels and milestone assignments before launching. This setup requirement is documented in `docs/orchestration.md`.
- Future autonomy levels (e.g., `supervised` with human-approval gates) can be added without breaking existing mode values.
