# ADR-001: gh-First GitHub Operations with MCP Fallback

**Status:** Accepted (revised 2026-05-02)
**Date:** 2024-01-01, revised 2026-04-08, revised 2026-05-02
**Deciders:** Code Looper project team

## Context

Code Looper orchestrates agent CLIs that can make GitHub mutations
(create/update issues, open/comment on PRs, merge branches). There are
two ways mutations could be performed:

1. **`gh` CLI** — the GitHub CLI tool, available in most dev environments
2. **MCP server tool calls** via the configured GitHub MCP server

An earlier revision of this ADR (2026-04-08) scoped MCP-only to agent
prompts while permitting the engine itself to use `gh` freely. Experience
showed that requiring MCP server configuration as a default prerequisite
added friction and token cost without meaningful auditability gains over
the existing structured logs.

## Decision

**Default policy is `gh`-first with MCP fallback.**

- Every prompt sent to a provider is augmented with `GITHUB_CLI_FIRST_PREAMBLE`
  (in `src/policy_guard.rs`) instructing the agent to use `gh` CLI first for
  GitHub operations and fall back to configured MCP tools when `gh` is
  unavailable or fails for tool-capability reasons.
- The **engine** (this module and [`crate::issue_tracker`]) always uses `gh`
  directly for its own bookkeeping: opening/merging PRs, posting lifecycle
  comments, and merge-cleanup branch operations. These actions are recorded in
  the run manifest and structured logs.
- `allow_direct_github = true` is the runtime default. Setting
  `allow_direct_github = false` switches agent prompts to strict MCP-only
  write policy (`MCP_ONLY_PREAMBLE`) and enables the `.mcp.json` prerequisite
  check at startup.
- At startup in strict mode, `workspace::check` looks for a `"github"` entry
  in the workspace `.mcp.json`. In default mode this check is skipped.

## Consequences

**Positive:**
- Removes the MCP server as a required dependency for the common case.
- Reduces token cost: `gh` CLI calls are cheaper and faster than MCP round-trips.
- Engine and agent both default to the same tool (`gh`), eliminating the
  prior dual-path confusion.
- Strict MCP-only mode remains available as an opt-in for environments that
  require fully audited MCP mutations.

**Negative:**
- Prompt-level enforcement only; a misbehaving agent could ignore the preamble.
  A stronger sandbox would require running provider processes without `gh` on
  `PATH`, which is out of scope.
- `gh` must be available in the environment; CI or locked-down sandboxes that
  lack `gh` should set `allow_direct_github = false` and configure MCP.

## Non-goals

- Routing engine bookkeeping through MCP. Engine actions are audited via
  structured logs and the per-run manifest.
- Runtime health-probing of the MCP server at startup.

## References

- `src/policy_guard.rs` — `augment_prompt` enforcement
- `src/pr_manager.rs` — engine `gh` usage for PR lifecycle
- `src/issue_tracker.rs` — engine `gh` usage for issue bookkeeping
- `src/workspace.rs` — startup `.mcp.json` configuration check (strict mode only)
- #219 — tracking issue for this revision
