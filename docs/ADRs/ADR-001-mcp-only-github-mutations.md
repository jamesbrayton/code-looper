# ADR-001: MCP-Only GitHub Mutation Path

**Status:** Accepted  
**Date:** 2024-01-01  
**Deciders:** Code Looper project team

## Context

Code Looper orchestrates agent CLIs that can make GitHub mutations (create/update issues, open/comment on PRs, merge branches). There are two ways mutations could be performed:

1. **Direct REST API calls** via the `gh` CLI `api` subcommand or raw HTTP
2. **MCP server tool calls** via the configured GitHub MCP server

Using raw CLI or REST calls bypasses structured tool tracking, makes mutations harder to audit, and creates a dual-path problem where some mutations are observable by MCP and others are not.

## Decision

All GitHub read and write operations in orchestration flows **must** go through MCP server tool interfaces. Direct `gh api` calls and raw REST mutations are disabled by default. The `PolicyGuard` layer enforces this at runtime.

A `--allow-direct-github` unsafe override flag exists for debugging and testing but is documented as unsupported for production use.

## Consequences

**Positive:**
- All GitHub mutations are observable and auditable through a single channel (MCP).
- Reduces risk of accidental over-automation (e.g., mass issue creation).
- Easier to mock/stub for testing — tests can inject fake MCP tool responses.
- Aligns with the PRD success criterion that 100% of GitHub mutations use MCP.

**Negative:**
- Requires the GitHub MCP server to be configured and reachable at startup.
- Loop startup performs a capability check (`workspace::check`) that fails fast when MCP is not available.
- Adds latency relative to direct REST calls (MCP round-trip overhead).

## References

- `src/policy_guard.rs` — `PolicyGuard` enforcement
- `src/workspace.rs` — startup capability validation
- PRD §2 Acceptance Criteria: "GitHub operations and prerequisites"
