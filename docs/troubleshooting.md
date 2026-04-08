# Troubleshooting

Common failure modes, their causes, and specific remediation steps.

---

## Provider CLI not found

**Symptom**

```
error: provider process spawn failed: No such file or directory (os error 2)
```

**Cause**

The selected provider binary (`claude`, `gh`, or `codex`) is not on `$PATH`.

**Remediation**

| Provider | Install command |
|----------|----------------|
| Claude Code | `npm install -g @anthropic-ai/claude-code` |
| GitHub Copilot | `gh extension install github/gh-copilot` (requires `gh` first) |
| Codex | `npm install -g @openai/codex` |

After installing, verify the binary is accessible:
```bash
which claude    # or: gh copilot --help  or: codex --version
```

---

## Missing MCP config

**Symptom**

```
[mcp-github-server] No .mcp.json found in '<dir>'
  → Remediation: Create .mcp.json at the repository root ...
```

Or the loop starts but GitHub orchestration calls fail mid-run.

**Cause**

`.mcp.json` is absent or does not contain a `"github"` key.

**Remediation**

See [Workspace Prerequisites — mcp-github-server](workspace-prerequisites.md#check-mcp-github-server) for the minimal `.mcp.json` template and `GITHUB_TOKEN` setup.

---

## Missing instruction file

**Symptom**

```
[instruction-file] No instruction file found in '<dir>'
```

**Cause**

None of `CLAUDE.md`, `AGENTS.md`, or `.github/copilot-instructions.md` exist in the workspace root.

**Remediation**

Create `CLAUDE.md` at the repository root. See [Workspace Prerequisites — instruction-file](workspace-prerequisites.md#check-instruction-file) for a template.

---

## `GITHUB_TOKEN` not set or expired

**Symptom**

```
authentication error: ...
```

Or orchestration flows silently return empty PR/issue lists when repos have open items.

**Cause**

`GITHUB_TOKEN` is missing, expired, or lacks the required scopes.

**Remediation**

1. Generate a personal access token at `https://github.com/settings/tokens` with `repo`, `issues`, and `pull_requests` scopes.
2. Export it:
   ```bash
   export GITHUB_TOKEN=ghp_...
   ```
   Or add it to a `.env` file in the repository root (`.env` is gitignored by default in Code Looper projects):
   ```
   GITHUB_TOKEN=ghp_...
   ```
3. Re-run `gh auth login` if using Copilot CLI, since it uses the `gh` credential store separately.

---

## Retry exhaustion

**Symptom**

```
warn: iteration N failed after M retries; ...
```

The loop continues but logs warnings for each failed iteration.

**Cause**

The provider process exits non-zero on every attempt within an iteration. Common reasons:
- The prompt causes the agent to hit a quota or rate limit.
- The agent exits early due to a tool error or missing context.
- A transient network error during a long-running operation.

**Remediation**

- Increase `--max-retries` and/or `--retry-backoff-ms` for transient failures.
- Use `--stop-on-failure` if persistent failures mean the remaining iterations have no value.
- Check the per-iteration transcripts in `.code-looper/runs/<run-id>/iteration-<n>.log` for the agent's output and error messages (one log per iteration).
- Review `--prompt-inline` / `--prompt-file` for ambiguities the agent might be interpreting as errors.

---

## Orchestration selects wrong workflow branch

**Symptom**

The loop runs but picks the `BacklogDiscovery` branch when open issues or PRs exist, or vice versa.

**Cause**

- `--repo-owner` or `--repo-name` is set incorrectly; the engine is reading a different repository than intended.
- The GitHub MCP server cannot authenticate (see `GITHUB_TOKEN` section above).
- Open items have labels like `do-not-loop` or `wip` that cause them to be skipped.

**Remediation**

1. Confirm `--repo-owner` and `--repo-name` match the target repository exactly (case-sensitive).
2. Verify the token can access the repo:
   ```bash
   gh issue list --repo owner/name
   ```
3. Check for skip labels on PRs/issues: in `multi-pr` mode, PRs labeled `do-not-loop` or `wip` are skipped. Remove those labels or adjust `pr_management.skip_labels` in config.
4. Enable debug logging to trace the policy decision:
   ```bash
   code-looper --log-level debug ...
   ```

---

## Loop runs forever (`--iterations -1`) and consumes disk space

**Symptom**

Run artifacts accumulate in `.code-looper/runs/` and disk usage grows.

**Cause**

By default the loop retains the 10 most recent run directories. With very long-running or high-frequency loops, each run's transcript can be large.

**Remediation**

- Lower `--keep-runs` (e.g., `--keep-runs 3`) to prune older runs more aggressively.
- Use `--no-summary` to suppress the markdown summary artifact.
- Point `--artifacts-dir` at a path on a larger volume.
- For CI environments where artifacts are not needed, combine `--no-summary` with `--keep-runs 1`.

---

## `auto_close_owned_issues` warning at end of run

**Symptom**

```
warn: owned issue #N is still open at end of run. Set auto_close_owned_issues=true to close automatically.
```

**Cause**

The loop finished all iterations but the linked issue (set via `--comment-issue`) is still open. The agent was prompted to close it but did not.

**Remediation**

- Enable `auto_close_owned_issues = true` in `[issue_tracking]` to have the engine close the issue automatically with a completion comment.
- Or close the issue manually and mark it `completed`.

---

## Run artifacts missing after an interrupted run

**Symptom**

`.code-looper/runs/<run-id>/` exists but has no `manifest.json` or `summary.md`.

**Cause**

The process was killed (Ctrl-C, OOM, signal) before the end-of-run artifacts were written. The transcript file may still be present, written incrementally.

**Remediation**

The transcript (`transcript.txt`) is written per-iteration and is safe to read after an interrupted run. The manifest and summary are written only at clean run exit. Re-run the loop to get a complete artifact set for the next run.
