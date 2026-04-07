# ADR-002: Provider Adapter Trait Design

**Status:** Accepted  
**Date:** 2024-01-01  
**Deciders:** Code Looper project team

## Context

Code Looper supports multiple agent CLIs (Claude Code, GitHub Copilot, Codex) and must allow new providers to be added without changing core loop logic. There are several integration patterns:

1. **Enum dispatch** — a single `Provider` enum with match arms per CLI; easy to add, but `loop_engine.rs` must be changed for every new provider.
2. **Trait objects** — a `ProviderAdapter` trait with `dyn` dispatch; new providers only require a new `impl` block and a match arm in the factory function.
3. **Plugin/shared library** — external `.so`/`.dll` loaded at runtime; maximum extensibility, but significant complexity and safety risk.

## Decision

Use **trait objects** (`Box<dyn ProviderAdapter>`). The `ProviderAdapter` trait exposes:
- `fn name(&self) -> &str` — display name for logging
- `fn execute(&self, prompt: &str) -> Result<ExecutionResult, LooperError>` — run one iteration

The `build_adapter()` factory in `src/provider.rs` maps the `Provider` config enum to the concrete type. Adding a new provider requires:
1. Implement `ProviderAdapter` for the new struct.
2. Add an arm to the `build_adapter` match.
3. Add a variant to the `Provider` config enum.

The PRD success criterion ("new provider scaffold implementable in ≤ 1 developer day") is satisfied by this design.

## Consequences

**Positive:**
- Loop engine is fully decoupled from provider implementation details.
- Easy to inject `FakeAdapter` in tests without touching production code.
- Output redaction (`security::redact_secrets`) and streaming apply uniformly through the shared `run_provider_process` helper.

**Negative:**
- Slight runtime overhead from vtable dispatch (negligible vs. provider subprocess latency).
- Provider-specific flags/options that don't fit the `execute(prompt)` interface require encoding in the struct fields rather than being exposed as typed parameters.

## References

- `src/provider.rs` — trait definition and all three adapter impls
- `src/error.rs` — `LooperError` variants used for provider failures
- PRD §2 Success Criteria: "New provider adapter scaffold can be implemented and validated in ≤ 1 developer day"
