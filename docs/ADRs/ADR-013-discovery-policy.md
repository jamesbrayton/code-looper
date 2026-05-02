# ADR-013: Discovery Policy

**Status:** Accepted
**Date:** 2026-05-02
**Deciders:** Code Looper project team
**Related:** #191

## Context

During execution, the agent often discovers work that was not anticipated when the milestone was set up — bugs found while implementing, missed dependencies, scope gaps. The orchestration engine must define a policy for where this discovered work lands.

Two competing concerns:
1. **Quality first:** Shipping with known open issues is worse than a delayed release. Discovered work should block the current milestone.
2. **Velocity first:** Scope creep delays releases. Discovered work should never block the current milestone.

## Decision

Three discovery policies, configurable via `orchestration.discovery.policy`:

| Policy | Behavior |
|--------|---------|
| `add-to-milestone` | Discovered work is added to the current milestone (default) |
| `defer` | Discovered work is always pushed to the next milestone |
| `prompt` | The agent must ask the user before adding or deferring |

`add-to-milestone` is the **default** because:
- For an automation tool, release quality is more important than release date.
- A milestone that closes with known open bugs provides false confidence.
- Users who need velocity can override to `defer`.

`defer` is the escape valve for teams in a deliberate sprint/velocity mode where scope changes require human decision.

`prompt` is available but not recommended for long-running autonomous loops, as it would block the loop waiting for user input.

## Consequences

- The discovery policy is injected into the execution lifecycle prompt so agents know where to create discovered issues.
- `add-to-milestone` requires the agent to know the current milestone number; this is passed via the prompt template.
- `defer` requires the agent to know a "next milestone" target; if none exists, the agent should create one or leave the issue without a milestone.
- The engine does not mechanically enforce the policy; it communicates it via prompt and relies on agent compliance.
