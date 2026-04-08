# ADR-001: MCP-Only GitHub Mutations for Agent Prompts

**Status:** Accepted (revised 2026-04-08)
**Date:** 2024-01-01, revised 2026-04-08
**Deciders:** Code Looper project team

## Context

Code Looper orchestrates agent CLIs that can make GitHub mutations
(create/update issues, open/comment on PRs, merge branches). There are
two ways mutations could be performed:

1. **Direct REST API calls** via the `gh` CLI `api` subcommand or raw HTTP
2. **MCP server tool calls** via the configured GitHub MCP server

Using raw CLI or REST calls from inside an **agent** bypasses structured
tool tracking, makes agent-driven mutations harder to audit, and creates
a dual-path problem where some agent-driven mutations are observable by
MCP and others are not.

In contrast, the **engine itself** — the Rust process that spawns
provider CLIs, manages branches, merges PRs, and posts lifecycle
comments — is the audit trail. Its actions are already observable via
structured logs, run artifacts, and the session summary. Routing the
engine's own bookkeeping through MCP would add latency, introduce a new
transport dependency on an agent tool interface, and does not improve
auditability over the existing telemetry.

## Decision

**Scope:** MCP-only applies to GitHub mutations performed by **agent
prompts**, not to subprocess calls issued by the engine itself.

- Every prompt sent to a provider is augmented with a preamble
  (`MCP_ONLY_PREAMBLE` in `src/policy_guard.rs`) instructing the agent
  to use only MCP server tools for GitHub writes. This is the
  enforcement point for agent behaviour.
- The **engine** is permitted to invoke `gh` (or, in future, direct REST
  calls) for its own bookkeeping: opening/merging PRs
  (`src/pr_manager.rs`), posting lifecycle comments
  (`src/issue_tracker.rs`), and merge-cleanup branch operations. These
  actions are recorded in the run manifest and structured logs, and are
  the canonical audit channel for engine behaviour.
- `--allow-direct-github` disables the agent-prompt preamble. It does
  **not** affect engine subprocess calls, which are always permitted.
- At startup, `workspace::check` looks for a `"github"` entry in the
  workspace `.mcp.json`. This is a best-effort configuration sanity
  check, not a runtime health probe — the engine does not attempt to
  ping the MCP server before running.

## Consequences

**Positive:**
- Agent-driven GitHub mutations flow through a single, observable
  channel (MCP).
- Engine bookkeeping stays on the lowest-latency, lowest-dependency
  path (`gh` CLI) and is audited via structured logs.
- Easier to mock/stub for testing — tests substitute engine `gh` calls
  at the adapter layer and inject fake MCP responses for agent
  behaviour.
- Removes the contradiction between the PolicyGuard preamble (which
  told agents `gh` is forbidden) and the `pr_manager` module docstring
  (which called `gh` the approved mutation path).

**Negative:**
- The single-preamble design means a misbehaving agent could still run
  `gh` directly; enforcement is prompt-level, not sandbox-level. Agents
  are trusted to honour the preamble. A stronger sandbox would require
  running provider processes without `gh` on `PATH`, which is out of
  scope for this ADR.
- Engine-side `gh` calls remain a failure surface that is not exercised
  through MCP tests. These are covered by targeted integration tests in
  `src/pr_manager.rs` and `src/issue_tracker.rs` instead.

## Non-goals

- Routing engine bookkeeping through MCP. This was considered and
  rejected for the reasons in the Context section. If a future version
  of the project wants full MCP audit of engine actions, that is a new
  ADR with its own cost/benefit analysis.
- Runtime health-probing of the MCP server at startup. The current
  `workspace::check` only verifies that `.mcp.json` mentions a
  `"github"` entry.

## References

- `src/policy_guard.rs` — `augment_prompt` enforcement (the sole
  MCP-only enforcement point)
- `src/pr_manager.rs` — permitted engine `gh` usage for PR lifecycle
- `src/issue_tracker.rs` — permitted engine `gh` usage for issue
  bookkeeping
- `src/workspace.rs` — startup `.mcp.json` configuration check
- PRD §2 Acceptance Criteria (to be updated alongside this ADR to scope
  "100% of GitHub mutations use MCP" to agent-initiated mutations)
- #59 — review discussion that prompted the revision
