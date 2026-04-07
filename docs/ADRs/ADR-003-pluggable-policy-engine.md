# ADR-003: Pluggable Orchestration Policy Engine

**Status:** Accepted  
**Date:** 2024-01-01  
**Deciders:** Code Looper project team

## Context

The loop engine needs to route work to the right workflow (PR review, issue execution, backlog discovery) based on live repository state. Early versions used a hardcoded `if open_prs → pr-review, else if open_issues → issue-execution, else backlog-discovery` chain. This worked for the MVP but:

- Rules could not be customized per project without recompiling.
- Testing required mocking the entire policy evaluation.
- The ordering and conditions were opaque to users.

## Decision

Introduce a **pluggable policy rules model** (`src/config.rs` `PolicyRule` / `PolicyCondition` / `PolicyWorkflow`) where:

1. Default rules are embedded in code (`default_policy_rules()`) and match the MVP behavior.
2. Users can override or extend rules via the `[orchestration.policies]` section of `loop.toml`.
3. The `PolicyEngine` evaluates rules in order, returning the first matching `WorkflowBranch`.
4. Context resolution is abstracted behind `ContextResolver` trait so tests can inject `StubContextResolver` without calling `gh`.

Rules are defined as:

```toml
[[orchestration.policies]]
condition = "has_open_prs"
workflow  = "pr-review"

[[orchestration.policies]]
condition = "has_open_issues"
workflow  = "issue-execution"

[[orchestration.policies]]
condition = "always"
workflow  = "backlog-discovery"
```

## Consequences

**Positive:**
- Projects can invert priority (issues before PRs) or add no-op policies via config alone.
- Default behavior is fully specified as data, making it easy to audit and test.
- `ContextResolver` abstraction enables deterministic unit tests for every policy branch.

**Negative:**
- More config surface area for users to learn.
- Rule evaluation is linear (first match wins); complex OR/AND conditions require multiple rules.
- Custom condition types beyond `has_open_prs`, `has_open_issues`, and `always` require code changes.

## References

- `src/orchestration.rs` — `PolicyEngine`, `ContextResolver`, `WorkflowBranch`
- `src/config.rs` — `PolicyRule`, `PolicyCondition`, `PolicyWorkflow`, `default_policy_rules`
- PRD §2 Acceptance Criteria: "Orchestration engine — conditional policy chain"
