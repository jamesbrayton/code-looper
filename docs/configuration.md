# Configuration Reference

Code Looper can be configured through a config file (TOML or YAML), CLI flags, or a combination of both. CLI flags always take precedence over values in the config file.

## Precedence

```
CLI flags  >  config file (TOML or YAML)  >  built-in defaults
```

## Loading a config file

Pass `--config path/to/config.toml` (or `.yaml` / `.yml`) to load a base configuration. Any CLI flag explicitly set on the same invocation overrides the corresponding value from the file.

The format is detected automatically from the file extension:

| Extension | Format |
|-----------|--------|
| `.yaml`, `.yml` | YAML |
| `.toml` or anything else | TOML (default) |

**TOML example (`looper.toml`):**

```toml
provider = "claude"
iterations = 5
log_level = "info"
```

**YAML equivalent (`looper.yaml`):**

```yaml
provider: claude
iterations: 5
log_level: info
```

---

## Top-level fields

| TOML key | CLI flag | Type | Default | Description |
|----------|----------|------|---------|-------------|
| `provider` | `--provider` | `claude` \| `copilot` \| `codex` | `claude` | Agent CLI to use for each iteration |
| `iterations` | `--iterations` | integer or `-1` | `1` | How many iterations to run; `-1` runs forever |
| `prompt_inline` | `--prompt-inline` | string | — | Prompt text passed directly (mutually exclusive with `prompt_file`) |
| `prompt_file` | `--prompt-file` | path | — | Path to a markdown file whose contents become the prompt (mutually exclusive with `prompt_inline`) |
| `log_level` | `--log-level` | `trace`\|`debug`\|`info`\|`warn`\|`error` | `info` | Tracing log level |
| `workspace_dir` | `--workspace-dir` | path | cwd | Directory to use as the workspace root for prerequisite checks |
| `skip_prereq_check` | `--skip-prereq-check` | bool | `false` | Skip instruction-file and MCP config validation at startup |
| `allow_direct_github` | `--allow-direct-github` | bool | `false` | **Unsafe.** Allow GitHub access via `gh` CLI instead of requiring MCP |
| `stop_on_failure` | `--stop-on-failure` | bool | `false` | Stop the loop after the first iteration that fails after all retries |
| `max_retries` | `--max-retries` | integer | `0` | Additional retry attempts per iteration on non-zero exit |
| `retry_backoff_ms` | `--retry-backoff-ms` | integer | `500` | Base delay in milliseconds between retry attempts |
| `retry_backoff_multiplier` | `--retry-backoff-multiplier` | float | `1.0` | Exponential backoff multiplier. `1.0` = flat; `2.0` = doubles delay each retry. Delay for attempt N = `retry_backoff_ms × multiplier^(N-1)` |
| `on_complete` | `--on-complete` | string | — | Shell command to run once after the loop finishes (runs via `sh -c`) |
| `provider_extra_args` | `--provider-extra-arg` (repeatable) | list of strings | `[]` | Extra arguments appended to the provider CLI invocation, after the adapter's hardcoded flags and before the prompt. Each element is a separate arg (no shell expansion). |

### Prompt validation

- `--prompt-inline` and `--prompt-file` are mutually exclusive. Passing both is a validation error.
- When orchestration is enabled, a prompt is generated automatically; providing `--prompt-inline` or `--prompt-file` alongside `--orchestration` is still valid — the user prompt is appended to the generated preamble.

---

## `[orchestration]`

Controls the policy engine that selects a workflow branch per iteration from repository context.

| TOML key | CLI flag | Type | Default | Description |
|----------|----------|------|---------|-------------|
| `orchestration.enabled` | `--orchestration` | bool | `false` | Enable the policy engine |
| `orchestration.repo_owner` | `--repo-owner` | string | — | GitHub owner (user or org); required when `enabled = true` |
| `orchestration.repo_name` | `--repo-name` | string | — | GitHub repository name; required when `enabled = true` |
| `orchestration.policies` | — | array of `PolicyRule` | see below | Ordered policy rule chain evaluated each iteration |

### `[[orchestration.policies]]` — pluggable policy rules

Each rule in the array specifies a condition and the workflow to execute when that condition is met. Rules are evaluated **in order**; the first matching rule wins.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `condition` | string | yes | `has_open_prs` · `has_open_issues` · `always` |
| `workflow` | string | yes | `pr-review` · `issue-execution` · `backlog-discovery` |
| `prompt_override` | string | no | Custom prompt for this rule; overrides the built-in default for the selected workflow |

**Default policy chain** (used when `policies` is omitted):

```toml
[[orchestration.policies]]
condition = "has_open_prs"
workflow = "pr-review"

[[orchestration.policies]]
condition = "has_open_issues"
workflow = "issue-execution"

[[orchestration.policies]]
condition = "always"
workflow = "backlog-discovery"
```

**Custom example** — skip PR review entirely and only work on issues with a custom prompt:

```toml
[[orchestration.policies]]
condition = "has_open_issues"
workflow = "issue-execution"
prompt_override = "Focus only on bugs labelled `critical` this iteration."

[[orchestration.policies]]
condition = "always"
workflow = "backlog-discovery"
```

> **Note:** If no rule matches (e.g. you define only `has_open_prs` but there are no open PRs), the engine returns an error. Always include an `always` fallback rule.

---

## `[issue_tracking]`

Controls issue tracking and run-lifecycle commenting.

| TOML key | CLI flag | Type | Default | Description |
|----------|----------|------|---------|-------------|
| `issue_tracking.mode` | `--issue-tracking-mode` | `github` \| `local` | `local` | Backend for issue tracking |
| `issue_tracking.repo_owner` | `--issue-tracking-owner` | string | — | GitHub owner for issue tracking; falls back to `--repo-owner` |
| `issue_tracking.repo_name` | `--issue-tracking-repo` | string | — | GitHub repo name for issue tracking; falls back to `--repo-name` |
| `issue_tracking.local_promise_path` | `--local-promise-path` | path | `.code-looper/promise.md` | Path to local promise file when mode is `local` |
| `issue_tracking.comment_issue_number` | `--comment-issue` | integer | — | GitHub issue number to post run-lifecycle comments on |
| `issue_tracking.comment_cadence` | `--comment-cadence` | `milestones`\|`every-iteration`\|`off-engine` | `milestones` | How often the engine posts issue comments |
| `issue_tracking.auto_close_owned_issues` | `--auto-close-owned-issues` | bool | `false` | Close the linked issue at end-of-run if the agent left it open |
| `issue_tracking.standard_labels` | — | string array | `["bug","enhancement","tech-debt","discovered-during-loop"]` | Labels the engine ensures exist on the repo at startup (GitHub mode only) |

### Comment cadence values

| Value | Behavior |
|-------|----------|
| `milestones` | Comment at run start, run end, blockers, and failed iterations |
| `every-iteration` | Comment after every iteration regardless of outcome |
| `off-engine` | Engine never posts comments; the agent is still prompted to do so |

---

## `[pr_management]`

Controls feature branch and pull-request lifecycle.

| TOML key | CLI flag | Type | Default | Description |
|----------|----------|------|---------|-------------|
| `pr_management.mode` | `--pr-mode` | `no-pr`\|`single-pr`\|`multi-pr` | `no-pr` | PR strategy |
| `pr_management.base_branch` | `--base-branch` | string | `main` | Branch to open PRs into |
| `pr_management.branch_prefix` | `--branch-prefix` | string | `loop/` | Prefix for feature branches created by the loop |
| `pr_management.require_human_review` | `--require-human-review` / `--no-require-human-review` | bool | `true` | When true, the loop never merges a PR itself |
| `pr_management.allow_force_push` | — | bool | `false` | Allow force-push with `--force-with-lease` on feature branches |
| `pr_management.ready_marker` | — | string | — | Sentinel string in agent output that triggers PR creation |
| `pr_management.triage_priority` | — | `oldest`\|`newest`\|`least-conflicts` | `oldest` | PR ordering for `multi-pr` triage; `least-conflicts` prioritises MERGEABLE PRs |
| `pr_management.skip_labels` | — | string array | `["do-not-loop","wip"]` | Labels that cause a PR to be skipped during `multi-pr` triage |

### PR mode values

| Value | Behavior |
|-------|----------|
| `no-pr` | Commit and push to a feature branch only; never open a PR |
| `single-pr` | Work on one feature branch; open a PR when work is shippable, then continue pushing to that branch until merged |
| `multi-pr` | On each iteration, triage open PRs first (review, fix, merge); open new feature branches for issue work only when no PR can be advanced |

---

## `[telemetry]`

Controls artifact collection and run summaries.

| TOML key | CLI flag | Type | Default | Description |
|----------|----------|------|---------|-------------|
| `telemetry.stream_output` | `--stream-output` / `--no-stream-output` | bool | `true` | Stream provider stdout/stderr to the terminal in real time |
| `telemetry.artifacts_dir` | `--artifacts-dir` | path | `.code-looper/runs` | Root directory for per-run artifact directories |
| `telemetry.keep_runs` | `--keep-runs` | integer | `10` | Number of most-recent run directories to retain |
| `telemetry.no_summary` | `--no-summary` | bool | `false` | Suppress the markdown summary and terminal summary at end of run |

---

## `[[multi_repo]]`

When one or more `[[multi_repo]]` entries are present, Code Looper runs the
configured loop for each target repository in sequence instead of the default
single-repo mode. Each entry is a separate TOML array item.

| TOML key | Type | Required | Description |
|----------|------|----------|-------------|
| `path` | path | yes | Filesystem path to the repository root. Relative paths are resolved from the working directory at startup. |
| `name` | string | no | Human-readable label used in run logs and the aggregate summary. Defaults to the last path component of `path`. |
| `prompt_override` | string | no | Per-repo prompt that replaces the top-level `prompt_inline` / `prompt_file` value for this specific target. |

**Example:**

```toml
[[multi_repo]]
path = "/home/dev/repos/service-a"
name = "service-a"

[[multi_repo]]
path = "/home/dev/repos/service-b"
prompt_override = "Run security audit only — do not change any code."
```

When `multi_repo` is non-empty the standard single-repo loop engine is skipped.
A per-repo summary and an aggregate total are printed after all targets finish.

---

## `serve` subcommand

`code-looper serve` starts a long-running TCP listener that accepts loop-run
requests over a newline-delimited JSON (JSON-lines) protocol. This is the v2.0
service / embedding mode.

```
code-looper serve [--port N] [--bind-addr ADDR]
```

| CLI flag | Default | Description |
|----------|---------|-------------|
| `--port` | `7979` | TCP port to listen on |
| `--bind-addr` | `127.0.0.1` | Address to bind to (loopback-only by default) |

The base `LoopConfig` is loaded from `--config` (or defaults) before the
listener starts. Per-request overrides are accepted in the request body.

### Protocol

The service speaks newline-delimited JSON (JSON Lines).  A client may send
multiple JSON requests over the same connection, one per line, and the
service replies with one JSON response per request, also one per line.  The
connection stays open until the client closes it or the service shuts down.

All responses use a consistent envelope:

- Success: `{"ok":true,"data":{...}}`
- Error:   `{"ok":false,"error":"..."}`

**Supported commands:**

| Request | Response |
|---------|---------|
| `{"cmd":"run","prompt":"…","provider":"…"}` | `{"ok":true,"data":{"ok":true,"exit_code":0,"stdout":"…","stderr":"…","duration_ms":123}}` |
| `{"cmd":"status"}` | `{"ok":true,"data":{"uptime_secs":42,"run_count":7,"success_count":6,"failure_count":1,"provider":"claude"}}` |
| `{"cmd":"shutdown"}` | `{"ok":true,"data":{"message":"shutting down"}}` — service exits after sending this |
| _(unknown / parse error)_ | `{"ok":false,"error":"…"}` |

**Example session (netcat):**

```bash
printf '%s\n%s\n' '{"cmd":"status"}' '{"cmd":"status"}' | nc 127.0.0.1 7979
# → {"ok":true,"data":{"uptime_secs":5,"run_count":0,"success_count":0,"failure_count":0,"provider":"claude"}}
# → {"ok":true,"data":{"uptime_secs":6,"run_count":0,"success_count":0,"failure_count":0,"provider":"claude"}}
```

---

## Example TOML config

```toml
provider = "claude"
iterations = -1
log_level = "info"
stop_on_failure = false
max_retries = 2
retry_backoff_ms = 500
retry_backoff_multiplier = 2.0
on_complete = "echo 'Loop finished' | tee -a loop.log"
provider_extra_args = ["--model", "claude-opus-4-5"]

[orchestration]
enabled = true
repo_owner = "acme"
repo_name = "my-project"

[[orchestration.policies]]
condition = "has_open_prs"
workflow = "pr-review"

[[orchestration.policies]]
condition = "has_open_issues"
workflow = "issue-execution"

[[orchestration.policies]]
condition = "always"
workflow = "backlog-discovery"

[issue_tracking]
mode = "github"
repo_owner = "acme"
repo_name = "my-project"
comment_issue_number = 42
comment_cadence = "milestones"
auto_close_owned_issues = false

[pr_management]
mode = "single-pr"
base_branch = "main"
branch_prefix = "loop/"
require_human_review = true

[telemetry]
stream_output = true
keep_runs = 20

# Optional: run against multiple repositories in sequence.
# When present, the single-repo path is skipped entirely.
[[multi_repo]]
path = "/home/dev/repos/service-a"
name = "service-a"

[[multi_repo]]
path = "/home/dev/repos/service-b"
prompt_override = "Apply the same lint fixes as service-a."
```
