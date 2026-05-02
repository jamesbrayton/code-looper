# Engine-Injected Prompts and Preambles

This reference documents every piece of content the Code Looper engine programmatically injects into provider prompts or posts to GitHub issues/PRs. Use it to understand exactly what context the agent sees and which templates drive lifecycle comments.

## Prompt injection order

When the engine builds the final prompt for a provider invocation, content is layered top-to-bottom:

1. **GitHub policy preamble** (from `PolicyGuard::augment_prompt`)
2. **Engine-generated workflow prompt** (from orchestration or PR triage)
3. **User-supplied prompt** (`--prompt-inline` / `--prompt-file` / policy rule `prompt_override`)

The GitHub policy preamble is always first. Everything else depends on the active mode.

> **Future: user rules.** A planned feature (#86) will add user-authored rule
> files that slot between the MCP preamble and the engine workflow prompt.
> The exact layering will be:
>
> 1. GitHub policy preamble
> 2. User global rules (`[rules].global`)
> 3. User workflow rules (`[rules.workflows].<branch>`)
> 4. Engine-generated workflow prompt
> 5. User-supplied iteration prompt

---

## Prompt injections

### 1. GitHub policy preamble

| | |
|---|---|
| **Source** | `src/policy_guard.rs` — `GITHUB_CLI_FIRST_PREAMBLE` and `MCP_ONLY_PREAMBLE` |
| **Applied by** | `PolicyGuard::augment_prompt()` |
| **When** | Every iteration and subcommand |
| **Override** | Set `allow_direct_github = false` for strict MCP-only write policy |

**Text:**

```
IMPORTANT - GitHub operations policy:
Use `gh` CLI as the default path for GitHub operations (issues, pull requests,
comments, branch operations, and merges). If a `gh` command is unavailable or
fails for a tool-capability reason, fall back to the configured GitHub MCP
tools for that action.
```

### 2. Workflow branch default prompts

Selected by the policy engine when orchestration is enabled (`--orchestration`). Each branch has a hard-coded default prompt that can be overridden per-rule in the config.

| | |
|---|---|
| **Source** | `src/orchestration.rs:33–64` — `WorkflowBranch::default_prompt()` |
| **Applied by** | `LoopEngine::run()` during orchestration branch selection |
| **When** | Each iteration when `orchestration.enabled = true` |
| **Override** | `prompt_override` field on any `[[orchestration.policies]]` rule |

#### 2a. PR Review (`pr-review`)

```
Review open pull requests in this repository. For each open PR, check the
diff, verify tests pass, and leave a constructive review comment using the
MCP GitHub tools. Do not merge without explicit approval.
```

#### 2b. Issue Execution (`issue-execution`)

```
Work on open GitHub issues in this repository. Pick the highest-priority
unassigned issue, understand the requirements, implement the changes, and
update the issue via MCP GitHub tools when done.

**Issue lifecycle rules:**
- Comment at meaningful milestones (plan finalised, first pass done, tests
  added, blocker found).
- If you discover work that is out of scope for the current issue, create a
  new GitHub issue (via MCP) with a clear title, body, and one of the
  standard labels: `bug`, `enhancement`, `tech-debt`, or
  `discovered-during-loop`. Post a cross-reference comment on both issues.
- When the issue checklist is fully checked and changes are committed, close
  the issue with a short summary comment explaining what was done.
```

#### 2c. Backlog Discovery (`backlog-discovery`)

```
Explore the repository codebase and identify areas for improvement: missing
tests, documentation gaps, refactoring opportunities, or potential new
features. Create GitHub issues for the most impactful opportunities using
the MCP GitHub tools. Use one of the standard labels for each new issue:
`bug`, `enhancement`, `tech-debt`, or `discovered-during-loop`.
```

### 3. PR triage prompts (multi-PR mode)

Dynamically generated when `pr_management.mode = "multi-pr"` and the triage engine selects a PR to act on.

| | |
|---|---|
| **Source** | `src/pr_manager.rs:913–936` — `PrTriage::evaluate_pr_state()` |
| **When** | Multi-PR mode, when a triaged PR has failing checks or requested changes |
| **Override** | No direct override. Use `pr_management.skip_labels` to skip specific PRs. |

#### 3a. Checks failing

```
The CI checks on PR #<number> («<title>») are failing. Check out the branch,
diagnose the root cause, fix it, commit, and push. Do not merge — the loop
engine will handle the merge when checks pass.
```

#### 3b. Changes requested

```
PR #<number> («<title>») has review comments requesting changes. Read each
review comment, address the feedback, commit the fixes, and push. After
pushing, reply to each resolved comment thread.
```

---

## Bootstrap instruction file section

The `code-looper bootstrap` command injects a section into the target repo's instruction file (e.g. `CLAUDE.md`). This is not a prompt injection per se — it shapes every agent interaction by living in the instruction file the provider reads at startup.

| | |
|---|---|
| **Source** | `src/bootstrap.rs:24–46` — `CLAUDE_MD_SECTION` constant |
| **Applied by** | `code-looper bootstrap` |
| **When** | One-time during workspace setup |
| **Override** | Edit the instruction file directly. The section is fenced by `<!-- code-looper begin -->` / `<!-- code-looper end -->` markers; subsequent bootstrap runs detect and skip it. |

**Text:**

```markdown
<!-- code-looper begin -->
## Code Looper

This repository is configured to run with Code Looper.

### GitHub mutation policy

All GitHub operations (issue create/update/comment, PR review/comment/merge,
branch actions) should use the `gh` CLI by default. If `gh` is unavailable or
fails for capability reasons, fall back to GitHub MCP tools for that action.

### Work-log discipline

During loop runs the agent should:
- Comment on the linked issue at meaningful milestones (scope clarified, first
  implementation pass complete, tests added, blocker found, handoff).
- Keep the issue body current (checklist, decisions, blockers/dependencies).
- Create new issues (via GitHub MCP) when discovered work falls outside the
  current issue's scope, using labels: `bug`, `enhancement`, `tech-debt`,
  `discovered-during-loop`.
- Close the issue with a summary comment when the checklist is complete and
  the work is committed.
<!-- code-looper end -->
```

---

## Issue lifecycle comments

The engine posts structured comments to the GitHub issue specified by `issue_tracking.comment_issue_number`. All templates are hard-coded. Posting frequency is controlled by `issue_tracking.comment_cadence`.

| Cadence setting | What is posted |
|----------------|---------------|
| `milestones` (default) | Run-start, run-end, blockers |
| `every-iteration` | All of the above, plus per-iteration outcome |
| `off-engine` | Nothing — agent manages its own comments |

### Run-start comment

| | |
|---|---|
| **Source** | `src/loop_engine.rs:447–452` |
| **When** | Loop initialization, cadence ≠ `off-engine` |

```
**Loop run started** — run-id: `<run_id>`, provider: `<provider>`,
iterations: `<count>`, prompt-source: `<source>`
```

### Per-iteration outcome comment

| | |
|---|---|
| **Source** | `src/loop_engine.rs:973–979` |
| **When** | After each iteration, cadence = `every-iteration` |

```
**Iteration <N>** — outcome: `<label>`, duration: <ms>ms[, retries: <N>][, error: `<excerpt>`]
```

Duplicate consecutive comments are suppressed (deduplication at `src/loop_engine.rs:980–989`).

### Blocker comments

Posted regardless of cadence when a fatal error occurs.

| Variant | Source | Template |
|---------|--------|----------|
| Spawn failure | `src/loop_engine.rs:792–795` | `**Blocker** — iteration <N> aborted: provider spawn failure. \`<msg>\`` |
| Fatal error | `src/loop_engine.rs:821–823` | `**Blocker** — iteration <N> aborted: fatal error. \`<msg>\`` |
| Stop-on-failure | `src/loop_engine.rs:998–1002` | `**Blocker** — iteration <N> failed and \`stop_on_failure\` is set; halting loop. Outcome: \`<outcome>\`` |

### Auto-close comment

| | |
|---|---|
| **Source** | `src/loop_engine.rs:1073–1077` |
| **When** | `auto_close_owned_issues = true` and loop completes |

```
Loop run `<run_id>` completed — closing issue automatically
(`auto_close_owned_issues=true`).
```

### Run-end summary comment

| | |
|---|---|
| **Source** | `src/loop_engine.rs:1150–1157` |
| **When** | Loop completion, cadence ≠ `off-engine` |

```
**Loop run finished** — iterations: <N>, successes: <N>, failures: <N>,
retries: <N>, termination: `<reason>`
```

---

## PR body and comment templates

### Auto-opened PR body

| | |
|---|---|
| **Source** | `src/pr_manager.rs:456–465` — `PrManager::pr_body()` |
| **When** | Agent emits the ready-for-review signal and no open PR exists |

```
Closes #<issue_number>

> This pull request was opened automatically by Code Looper.

## Agent summary

<agent_summary>   (included only when the signal carries a summary)
```

### PR update comment

| | |
|---|---|
| **Source** | `src/pr_manager.rs:529–535` |
| **When** | Agent emits the ready-for-review signal and an open PR already exists |

```
**Code Looper update** — agent signalled ready-for-review.

<agent_summary>   (included only when the signal carries a summary)
```

### Issue-to-PR cross-reference

| | |
|---|---|
| **Source** | `src/issue_tracker.rs:369–371` — `GhIssueTracker::link_issue_to_pr()` |
| **When** | A PR is opened and linked to a tracked issue |

```
Linked to pull request #<pr_number>.
```

---

## How user rules compose with engine prompts

User rules (configured via `[rules]` in the config file) are **prepended** to
the engine workflow prompt, not replacing it.  This ensures the GitHub policy and
workflow structure are always present while giving users a way to inject
standing instructions (coding standards, review checklists, domain context).

In default mode, that policy means `gh`-first with MCP fallback.

The full prompt layering is:

| Layer | Source | Customisable? |
|-------|--------|--------------|
| GitHub policy preamble (`gh`-first default) | `policy_guard.rs` | Set `allow_direct_github = false` for strict MCP-only writes |
| User global rules | `[rules].global` config path | Yes — user-authored markdown |
| User workflow rules | `[rules.workflows].<branch>` | Yes — user-authored markdown |
| Engine workflow prompt | `orchestration.rs` / `pr_manager.rs` | Per-rule `prompt_override` in config |
| User iteration prompt | `--prompt-inline` / `--prompt-file` | Yes |

### Implementation details

- **Injection point:** `src/loop_engine.rs`, after the PR triage plan override and
  before `guard.augment_prompt()`.  The function `config::load_rules_for_branch()`
  reads rule files and assembles the rules preamble.
- **Re-read each iteration:** rule files are re-read on every iteration so edits
  take effect without a restart.  Read errors are logged and the rule is skipped
  (the last good content is cached and used as a fallback).
- **Workflow keys:** `[rules.workflows]` keys are the kebab-case `PolicyWorkflow`
  enum variants (`"pr-review"`, `"issue-execution"`, `"backlog-discovery"`).
  Invalid keys are rejected at deserialization time.
- **Size limits:** soft warning at 16 KB, hard error at 64 KB to prevent
  accidental prompt bloat from paste-mistakes.
