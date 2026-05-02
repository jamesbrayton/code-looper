# ADR-012: Prompt as Scope Contract

**Status:** Accepted
**Date:** 2026-05-02
**Deciders:** Code Looper project team
**Related:** #191

## Context

When the orchestration engine runs in `autonomous` mode with the grooming and planning lifecycles active, it can create and assign issues to milestones. Without a scope anchor, the engine might expand scope indefinitely — creating issues for any improvement it notices regardless of the original intent.

## Decision

The **launch prompt** (either `--prompt-inline` text or the content of `--prompt-file`) is the **scope contract** for the entire run. The agent must use the prompt goal to judge whether discovered work belongs in the current milestone or should be deferred.

- Issues are the *work decomposition* for achieving the goal — they can grow as the agent refines its understanding.
- Milestones define what gets shipped — scope is anchored to the original prompt.
- The engine passes the launch prompt to every lifecycle, not just execution. Grooming and planning lifecycles receive it as context for scoping decisions.

## Consequences

- The launch prompt must be stored on `LoopEngine` and passed through to every lifecycle prompt template.
- Agents must be instructed (via the lifecycle prompt) to compare discovered work against the launch prompt goal before adding it to the current milestone.
- Users who want the agent to expand scope freely should use a broad launch prompt ("improve this repository") rather than a specific one.
- This is enforced by convention (prompt text), not by hard code; the engine cannot mechanically verify that an issue "serves the goal."
