# Provider Adapters

Code Looper supports three provider adapters out of the box. Each adapter implements a common `ProviderAdapter` trait: it spawns the underlying CLI with the resolved prompt, captures stdout/stderr, and returns a normalized `ExecutionResult`.

Select a provider with `--provider <name>` or set `provider = "<name>"` in your TOML config.

---

## Claude Code CLI (`claude`)

**CLI flag value:** `claude` (default)

### How it is invoked

```
claude -p --dangerously-skip-permissions "<prompt>"
```

- `-p` runs in headless/pipe mode (no interactive TUI).
- `--dangerously-skip-permissions` bypasses the interactive permission confirmation prompt so the loop can run unattended.

### Environment requirements

| Requirement | Notes |
|-------------|-------|
| `claude` on `$PATH` | Install from [claude.ai/code](https://claude.ai/code) or via npm: `npm install -g @anthropic-ai/claude-code` |
| Anthropic credentials | Log in once interactively with `claude` before running the loop; credentials are cached locally |

### Known limitations

- `--dangerously-skip-permissions` is required for unattended use. This means the agent may perform file writes and shell commands without per-action confirmation. Use orchestration policies and workspace prerequisite checks to constrain scope.
- Long-running prompts can time out if the provider process does not complete within the system's process timeout.

### Example

```bash
code-looper \
  --provider claude \
  --iterations 5 \
  --prompt-inline "Review open GitHub issues and prioritize the backlog."
```

---

## GitHub Copilot CLI (`copilot`)

**CLI flag value:** `copilot`

### How it is invoked

```
gh copilot suggest -t shell "<prompt>"
```

- Uses the GitHub CLI's `copilot suggest` subcommand with target type `shell`.
- Stdout contains the suggested shell command or explanation; Code Looper captures it verbatim.

### Environment requirements

| Requirement | Notes |
|-------------|-------|
| `gh` on `$PATH` | Install from [cli.github.com](https://cli.github.com) |
| GitHub Copilot CLI extension | Run `gh extension install github/gh-copilot` once |
| `gh auth login` | Must be authenticated with a Copilot-enabled GitHub account |

### Known limitations

- `gh copilot suggest -t shell` is designed for shell-command suggestions, not multi-step agentic execution. Expect shorter, more narrowly scoped responses compared to Claude Code.
- Does not natively support long context or file-read operations; complex prompt payloads may be truncated or produce lower-quality results.
- Copilot CLI requires an active internet connection and a GitHub account with Copilot access.

### Example

```bash
code-looper \
  --provider copilot \
  --iterations 3 \
  --prompt-inline "List the top 3 failing tests in this repo and suggest fixes."
```

---

## Codex CLI (`codex`)

**CLI flag value:** `codex`

### How it is invoked

```
codex "<prompt>"
```

- Passes the prompt as a positional argument to the `codex` binary.

### Environment requirements

| Requirement | Notes |
|-------------|-------|
| `codex` on `$PATH` | Install via npm: `npm install -g @openai/codex` |
| OpenAI API key | Set `OPENAI_API_KEY` in your environment or `.env` file |

### Known limitations

- Codex CLI is a community-maintained tool; its interface and behavior may change more frequently than Claude Code or Copilot CLI.
- Does not have built-in MCP tool support; all GitHub mutations must happen through prompt-guided `gh` CLI calls, which means `--allow-direct-github` may be required for orchestration flows.

### Example

```bash
export OPENAI_API_KEY=sk-...
code-looper \
  --provider codex \
  --iterations 2 \
  --allow-direct-github \
  --prompt-inline "Summarize recent commits and open a draft PR."
```

---

## Adding a new provider adapter

1. Add a new variant to `Provider` in `src/config.rs`.
2. Implement the `ProviderAdapter` trait in `src/provider.rs` (implement `name()` and `execute()`).
3. Wire the new variant in `build_adapter()` in `src/provider.rs`.
4. Add at least one test in the `provider::tests` module.

The adapter scaffold can be completed in under a day; see the PRD's "New provider adapter scaffold" success criterion.
