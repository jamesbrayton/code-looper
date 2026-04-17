# ADR-005: GitHub Issues + Milestones as Project Management Primitive

**Status:** Accepted
**Date:** 2026-04-17
**Deciders:** Code Looper project team
**Related:** #190

## Context

Code Looper needs a project management system that:

1. Is queryable by the orchestration engine at runtime — the engine must be able to ask "what should I work on next?" by reading state from an external source
2. Is visible and editable by both human contributors and automated agents
3. Does not require a separate SaaS subscription or significant setup overhead
4. Can serve as the source of truth for release scope

The candidates considered were:

- **GitHub Projects v2** — Kanban-style board with custom fields and views. Rich UI, but GraphQL-only API; board state is not directly queryable via the `gh` CLI or REST API in a way that maps cleanly to orchestration decisions.
- **JIRA** — Industry-standard issue tracker with deep workflow features. Requires a separate subscription and external service dependency; far more machinery than this project needs.
- **GitHub Issues + Labels + Milestones** — Built into every GitHub repo. Fully queryable via REST API and `gh` CLI. Labels are first-class filter primitives. Milestones map naturally to releases.

## Decision

Use **GitHub Issues with labels and milestones** as the sole project management primitive for code-looper.

- **Issues** are the unit of work
- **Labels** encode lifecycle state (`ready-for-dev`, `in-progress`, `blocked`) and priority (`priority-high`, `priority-medium`, `priority-low`)
- **Milestones** define release scope — one milestone = one release
- No GitHub Projects board is used or required

The full label taxonomy and milestone conventions are documented in `docs/project-management.md`.

## Consequences

**Positive:**
- The orchestration engine can query issue state entirely via the GitHub MCP server tools already configured (no new integrations required)
- Human contributors and automated agents use the same interface — no separate tracking system to sync
- Milestones provide a natural, queryable release boundary: a milestone with zero open issues and zero open PRs is the condition that fires the release lifecycle
- Zero additional cost or external service dependencies

**Negative:**
- No state machine enforcement — label discipline relies on the engine (and human contributors) applying labels consistently. A mislabeled issue can confuse orchestration queries.
- No visual board unless GitHub Projects is layered on top later (which is explicitly not done here)
- Labels are repo-global — the taxonomy must be documented and agreed upon; it cannot be enforced by schema

## Tradeoffs Accepted

- **No enforcement** of the label state machine. An issue can have both `ready-for-dev` and `blocked` applied simultaneously if someone makes a mistake. The convention document (`docs/project-management.md`) is the authority; enforcement lives in the agent's grooming logic.
- **Implicit states** (backlog, done) are not represented by labels. "Backlog" is inferred from the absence of state labels; "done" is inferred from closed state. This keeps the label surface minimal but means you cannot filter for "backlog" as a label — you must filter for "open issues with no state label."

## Alternatives Rejected

- **GitHub Projects v2**: The GraphQL-only query model does not align with the MCP GitHub tools already in use. Adding a Projects integration would require either new MCP tools or raw GraphQL calls, neither of which is justified at this project scale.
- **JIRA**: External dependency, cost, and operational overhead are not warranted for a single-repo project.

## References

- `docs/project-management.md` — Full label taxonomy and milestone conventions
- ADR-006 — State label taxonomy (companion ADR)
- #190 — Issue that introduced this system
- #191 — Orchestration engine that consumes these labels as query signals
