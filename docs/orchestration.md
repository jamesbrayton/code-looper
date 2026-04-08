# Code Looper Orchestration

This document describes how the loop engine selects workflow branches, the shippable-signal protocol, and PR lifecycle management.

## Workflow branch selection

Each iteration the orchestration policy engine evaluates the current repository context and selects one of three workflow branches:

| Branch | When selected | Default prompt focus |
|--------|--------------|---------------------|
| `pr-review` | Open PRs exist | Review diffs, leave comments via MCP |
| `issue-execution` | Open issues exist (no open PRs) | Pick highest-priority issue, implement, update via MCP |
| `backlog-discovery` | No open PRs or issues | Identify improvements, create issues via MCP |

The engine resolves repository context once per iteration before selecting a branch.

## Issue lifecycle

When orchestration is enabled, the engine expects agents to actively manage GitHub Issues throughout the run.

### Standard labels

At startup (GitHub mode only) the engine ensures the following labels exist on the repository, creating any that are absent:

| Label | Purpose |
|-------|---------|
| `bug` | Defects found during iteration |
| `enhancement` | Feature requests or improvements |
| `tech-debt` | Technical debt identified during work |
| `discovered-during-loop` | New scope discovered while working on a different issue |

The set is configurable via `issue_tracking.standard_labels` in `looper.toml`.

### Issue creation (out-of-scope discovery)

When the agent discovers work that falls outside the current issue's scope it should:

1. Create a new issue via GitHub MCP with a descriptive title, body, and one or more standard labels.
2. Add `discovered-during-loop` to the new issue.
3. Leave a comment on the current issue linking to the newly created one.

This keeps the current iteration focused and makes discovered work discoverable.

### Issue closure

When the agent has completed the current issue's checklist *and* the work is committed (or a PR is open), it should close the issue with a summary comment via GitHub MCP.

The engine performs an end-of-run verification: after the loop finishes it checks whether the owned issue is still open.  Behaviour depends on the `auto_close_owned_issues` configuration flag:

| Setting | Behaviour |
|---------|-----------|
| `false` *(default)* | Log a warning and leave the issue open for human review |
| `true` | Close the issue automatically with `state_reason: completed` |

### Linking

- When a PR is opened for issue work, the PR body includes `Closes #<issue>` and the issue receives a back-reference comment.
- New issues created during a run include a link back to the originating issue in their body.

## Shippable signal protocol

The loop engine watches each iteration's stdout for a *shippable signal* — a marker the agent emits when it believes the current branch contains review-ready work.  When detected the engine opens (or updates) a pull request via the `gh` CLI.

### Form 1 — Sentinel line (recommended)

Include a line in your output that is **exactly** the sentinel string (no surrounding text on the same line).  The default sentinel is:

```
LOOPER_READY_FOR_REVIEW
```

Example agent output:

```
All tests pass.
LOOPER_READY_FOR_REVIEW
```

The sentinel is case-sensitive and matched after trimming leading/trailing whitespace.

### Form 2 — JSON block

Emit a single-line JSON object with the `looper` key set to `"ready-for-review"`.  Optionally include a `summary` key whose value appears in the PR body:

```json
{"looper":"ready-for-review","summary":"Implemented user authentication module"}
```

The JSON block must appear on its own line and must start with `{`.

### Signal priority

If both forms are present in the same output, the sentinel line takes priority (it is found first in the scan).

### Custom sentinel

Override the sentinel string in `looper.toml`:

```toml
[pr_management]
ready_marker = "MY_CUSTOM_SIGNAL"
```

## PR lifecycle

### On first detection (no open PR)

The engine opens a pull request:

- **Head**: the active feature branch (e.g. `loop/42-my-feature`)
- **Base**: `pr_management.base_branch` (default: `main`)
- **Title**: `[LOOPER] #<issue>: <issue title>`
- **Body**: links `Closes #<issue>`, optionally includes the agent's `summary`
- **Labels**: `code-looper`, `needs-review`

### On subsequent detections (PR already open)

When `require_human_review = true` (the default): the engine logs a
`BlockedOnHumanReview` event and takes no further automated action.  The PR
remains open awaiting a human reviewer.

When `require_human_review = false`: the engine appends a comment to the
existing PR with the agent's update summary.

### Human-review gating

`require_human_review = true` (default) means the loop engine **never** calls
`gh pr merge` itself.  Merge is left entirely to a human reviewer.  This is the
safe default for all production usage.

To enable automated merging (advanced, use with care):

```toml
[pr_management]
require_human_review = false
```

## Multi-PR triage workflow

When `mode = "multi-pr"`, the engine runs a **triage step** at the start of
each iteration before invoking the agent.

### PR discovery

Open PRs are discovered by the `code-looper` label via `gh pr list`.

### State classification

Each candidate PR is classified into one of:

| State | Meaning | Engine action |
|-------|---------|---------------|
| `ChecksFailing` | One or more CI checks have `conclusion: FAILURE` | Agent is prompted to fix failures and push |
| `ChangesRequested` | `reviewDecision` is `CHANGES_REQUESTED` | Agent is prompted to address each review comment |
| `ReadyToMerge` | Approved (or no review required) and checks pass | Merge directly (when `require_human_review = false`) or report blocked |
| `NeedsReview` | Awaiting initial review | Skipped; engine proceeds to next PR |
| `Skipped` | PR carries a skip label (`do-not-loop`, `wip`, …) | Ignored entirely |

### Triage priority

The `triage_priority` setting controls which PR is acted on first when multiple
actionable PRs exist:

| Value | Behaviour |
|-------|-----------|
| `oldest` *(default)* | Oldest PR first (ascending creation order) |
| `newest` | Newest PR first |
| `least-conflicts` | PRs with `MERGEABLE` GitHub state first; `UNKNOWN` second; `CONFLICTING` last. Fetches all PR states upfront to sort before selecting an action. |

### Fall-through

If no open PR can be advanced (all skipped, all `NeedsReview`, or all blocked
on human review) the triage step reports this and the iteration proceeds as
normal issue-work.

### Prompt override

When an actionable PR is found, the engine **replaces** the normal iteration
prompt with a triage-generated prompt.  The agent is directed to:

- Fix failing CI checks (when `ChecksFailing`)
- Address reviewer feedback (when `ChangesRequested`)

The prompt includes the PR number and title so the agent can locate the correct
branch and PR.

## Configuration reference (`[pr_management]`)

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `mode` | `no-pr` \| `single-pr` \| `multi-pr` | `no-pr` | PR strategy |
| `base_branch` | string | `"main"` | Branch PRs target |
| `branch_prefix` | string | `"loop/"` | Feature branch prefix |
| `require_human_review` | bool | `true` | Gate merges on human approval |
| `allow_force_push` | bool | `false` | Allow `--force-with-lease` pushes |
| `ready_marker` | string | `"LOOPER_READY_FOR_REVIEW"` | Shippable sentinel |
| `triage_priority` | `oldest` \| `newest` \| `least-conflicts` | `oldest` | Multi-PR ordering |
| `skip_labels` | list of strings | `["do-not-loop","wip"]` | Labels that exclude a PR from triage |

## MCP-only policy

All GitHub **write** operations (PR create, PR comment, issue comment, label
edits, etc.) must flow through the configured GitHub MCP server tools.  This
is the Code Looper–approved mutation path: the policy guard layer prepends an
`MCP_ONLY_PREAMBLE` to every provider prompt that explicitly forbids direct
`gh` CLI write commands and raw GitHub REST API calls.

Read-only GitHub context (open issues, PR metadata, etc.) is gathered via the
GitHub MCP server by default.  The unsafe `--allow-direct-github` flag flips
the **read** path to use the `gh` CLI directly (via `GhCliContextResolver`)
*and* disables the MCP-only preamble — at which point write enforcement is no
longer applied to provider prompts.  Use only when you know the workspace has
no MCP server available and you accept the loss of write-path enforcement.
