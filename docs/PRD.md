# Product Requirements Document: Code Looper

## 1. Executive Summary

- **Problem Statement**: Current agent-loop automation is fragmented across one-off shell scripts that are tied to specific repositories, fixed workflows, and single-agent CLIs. This limits reuse, extensibility, and the ability to run more intelligent orchestration logic.
- **Proposed Solution**: Build a Rust CLI application, Code Looper, that provides a pluggable loop engine for multiple coding-agent CLIs (Claude Code, GitHub Copilot CLI, Codex CLI, and future adapters), configurable iteration behavior, and policy-driven orchestration flows.
- **Success Criteria**:
  - Time to configure and run first loop in a new repository is <= 10 minutes for a developer familiar with CLI tools.
  - Loop run completion rate is >= 95% across a benchmark of 100 scheduled iterations (excluding external provider outages).
  - At least 3 providers (Claude Code CLI, GitHub Copilot CLI, Codex CLI) are supported by stable adapters in v1.0.
  - New provider adapter scaffold can be implemented and validated in <= 1 developer day using documented extension interfaces.
  - Teams report >= 20% reduction in manual repetitive orchestration steps (self-reported survey and run-log evidence) within 60 days of adoption.
  - 100% of **agent-initiated** GitHub mutations (issue create/update/comment, PR review/comment/merge, branch actions) are executed through MCP server interfaces, enforced via a policy-guard preamble on every provider prompt. Engine-initiated bookkeeping calls (opening/merging PRs, posting lifecycle comments, merge-cleanup branch ops) are explicitly permitted to use the `gh` CLI directly and are audited through structured logs and the per-run manifest. See ADR-001 for the scoping rationale.

## 2. User Experience & Functionality

- **User Personas**:
  - Solo Developer: wants reliable autonomous loop execution without writing custom shell scripts per project.
  - Automation-Minded Engineer: wants configurable orchestration policies that route work based on repository state.
  - Platform Integrator (secondary): wants to embed Code Looper in higher-level automation workflows.

- **User Stories**:
  - As a solo developer, I want to run loop iterations against multiple agent CLIs so that I am not locked into one provider.
  - As an automation engineer, I want to set iteration count (including infinite mode with `-1`) and completion behavior so that runs can match my workflow constraints.
  - As a developer, I want to provide either an inline prompt or a prompt markdown file so that I can drive different loop goals without changing source code.
  - As an advanced user, I want conditional orchestration rules (for example, PR review path, issue execution path, product-discovery path) so that the loop can pick the right next action from repository context.
  - As an operator, I want detailed run logs and explicit exit states so that I can audit and debug loop behavior.
  - As a repository maintainer, I want workspace bootstrap mode that can set up required agent instructions and MCP config so that projects can adopt Code Looper with minimal manual setup.

- **Acceptance Criteria**:
  - Multi-provider execution:
    - System supports selecting provider via CLI flag (for example `--provider claude|copilot|codex`).
    - Provider adapters expose a common contract for prompt input, args, execution, and output capture.
    - Provider-specific failures produce normalized error codes and messages.
  - Loop control:
    - `--iterations` accepts positive integers and `-1` for continuous looping.
    - Loop can stop on success/failure policy and by manual interruption with graceful shutdown.
    - Optional completion promise/hook can be configured and executed at run end.
  - Prompt handling:
    - User can pass `--prompt-inline "..."` or `--prompt-file path.md`.
    - Validation prevents conflicting prompt options and missing files.
  - Orchestration engine:
    - Engine supports at least one conditional policy chain in MVP (if open PRs then review flow, else if open issues then issue flow, else backlog-discovery flow).
    - Context fetches are abstracted behind interfaces to allow future non-GitHub sources.
    - Policy execution model distinguishes between repository-level prerequisites (instruction files and skills) and Code Looper runtime controls.
  - Observability:
    - Every iteration creates structured logs with timestamp, provider, prompt source, decision path, command status, and duration.
    - Session summary includes counts of successes, failures, retries, skipped decisions, and termination reason.
  - GitHub operations and prerequisites:
    - All GitHub read and write operations in orchestration flows use MCP server tools; bypass paths are disabled unless explicitly enabled via an unsafe flag.
    - System validates workspace prerequisites at startup (for example presence/expected sections in instruction files and required skill references) and fails fast with remediation guidance when missing.
    - Optional `bootstrap` command can initialize or patch repository prerequisites (instruction files, MCP config stubs, and skill references) in an idempotent way.

- **Non-Goals**:
  - Building a hosted SaaS control plane in MVP.
  - Implementing a web UI in MVP.
  - Automatic issue creation for every possible PM opportunity with no human review.
  - Deep repository analytics or enterprise workflow integrations (Jira, Azure Boards, etc.) in MVP.
  - Automatic installation of external binaries/system packages without explicit user confirmation.

## 3. AI System Requirements

- **Tool Requirements**:
  - Provider adapters for:
    - Claude Code CLI
    - GitHub Copilot CLI
    - Codex CLI
  - Optional orchestration data sources:
    - GitHub MCP server for repository state checks and all mutation operations.
  - Configuration and execution tools:
    - TOML/YAML/CLI arg parser for run configuration.
    - Structured logging framework for machine-readable logs.
  - Workspace prerequisite tools:
    - Instruction file parser/patcher for repository policy files.
    - MCP configuration validator for required GitHub server availability.

- **Evaluation Strategy**:
  - Functional benchmarks:
    - Run matrix over providers x prompt mode x loop mode x orchestration policy.
    - Validate deterministic behavior for policy selection from mocked repository states.
  - Quality benchmarks:
    - Decision accuracy >= 95% against predefined orchestration test cases.
    - Retry logic recovers from transient provider failures in >= 90% of simulated cases.
  - Reliability benchmarks:
    - Long-run soak test: 12-hour loop with bounded memory growth and no crash.
  - Operator experience:
    - Log completeness score: 100% of runs must emit required audit fields.

## 4. Technical Specifications

- **Architecture Overview**:
  - CLI Layer:
    - Parses arguments and config, validates run options, initializes runtime.
  - Loop Engine:
    - Iteration scheduler, stop conditions, retry/backoff, completion hooks.
  - Provider Adapter Layer:
    - Standard trait/interface for execute, stream output, and normalize errors.
  - Orchestration Policy Engine:
    - Evaluates context, selects workflow branch, generates provider prompt payloads.
  - Policy Guard Layer:
    - Enforces MCP-only GitHub mutation path and blocks disallowed execution paths.
  - Workspace Bootstrap and Validation:
    - Checks/initializes repository prerequisites (instruction file fragments, skills references, MCP config hints) before loop execution.
  - Context Integrations:
    - GitHub MCP-backed resolver for PR/issue checks and write actions; abstraction for additional sources.
  - Telemetry and Logs:
    - Structured logs per iteration and session-level summaries.

- **Integration Points**:
  - External CLIs:
    - Claude Code CLI executable.
    - GitHub Copilot CLI executable.
    - Codex CLI executable.
  - GitHub:
    - All repository interactions are routed through GitHub MCP server tools.
    - Read-only checks and write actions (comment/review/merge/issue-create/update) are permission-gated by policy and execution mode.
    - Runtime performs capability checks at startup to verify required MCP tools are available.
  - Local filesystem:
    - Prompt markdown ingestion and log output directory.
    - Repository instruction files and skill references used for orchestration prerequisites.
    - Optional bootstrap writes are previewable via dry-run before apply.
  - Auth:
    - Provider credentials/token management through existing local auth mechanisms and environment variables.
    - MCP server authentication is expected to be preconfigured in the developer environment.

- **Security & Privacy**:
  - Secrets handling:
    - Never write raw tokens/secrets to logs.
    - Redact known token patterns in command output capture.
  - Command safety:
    - Explicit allowlist for provider executables and optional shell command constraints.
    - Deny-by-default policy for direct GitHub mutation via non-MCP command paths.
  - Data minimization:
    - Only capture context fields required for orchestration decisions.
  - Compliance baseline:
    - Maintain audit logs for run provenance (who/when/provider/policy) while avoiding sensitive prompt leakage by default.
  - Workspace integrity:
    - Bootstrap mode must preserve existing repository instruction content and use additive, clearly delimited updates.

## 5. Risks & Roadmap

- **Phased Rollout**:
  - MVP:
    - Rust CLI scaffolding and configuration model.
    - Loop engine with finite and infinite iteration modes.
    - Prompt source options (inline and file).
    - Adapters for Claude Code CLI, GitHub Copilot CLI, and Codex CLI.
    - Baseline conditional orchestration chain and structured logging.
    - MCP-only GitHub integration path with startup capability validation.
    - Prerequisite checker with actionable diagnostics.
  - v1.1:
    - Pluggable policy definitions via external config.
    - Improved retry/backoff and failure classification.
    - Enhanced GitHub workflow actions with guardrails.
    - Optional bootstrap subcommand for creating/updating repository prerequisites.
  - v2.0:
    - Additional provider adapters.
    - Multi-repo orchestration support.
    - Optional service mode and API surface for embedding.

- **Technical Risks**:
  - Provider CLI interface drift or breaking flag changes.
  - Non-deterministic agent outputs affecting policy reliability.
  - Long-running loop resource growth (memory/file handles/log volume).
  - Over-automation causing unintended repository actions.
  - Attribution challenge for productivity gains without careful telemetry design.
  - Drift between expected repository instruction/skill structure and real-world project variants.
  - MCP server unavailability or incompatible tool surface causing orchestration degradation.
