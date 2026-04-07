# Configuration Reference

Code Looper can be configured through a TOML file, CLI flags, or a combination of both. CLI flags always take precedence over values in the TOML file.

## Precedence

```
CLI flags  >  TOML config file  >  built-in defaults
```

## Loading a config file

Pass `--config path/to/config.toml` to load a base configuration. Any CLI flag explicitly set on the same invocation overrides the corresponding TOML value.

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
| `retry_backoff_multiplier` | — | float | `1.0` | Exponential backoff multiplier. `1.0` = flat; `2.0` = doubles delay each retry. Delay for attempt N = `retry_backoff_ms × multiplier^(N-1)` |
| `on_complete` | `--on-complete` | string | — | Shell command to run once after the loop finishes (runs via `sh -c`) |

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
| `issue_tracking.auto_close_owned_issues` | — | bool | `false` | Close the linked issue at end-of-run if the agent left it open |
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
| `pr_management.triage_priority` | — | `oldest`\|`newest`\|`least-conflicts` | `oldest` | PR ordering for `multi-pr` triage |
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
```
