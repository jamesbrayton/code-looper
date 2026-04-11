# Getting Started with Code Looper

This guide walks you through installing prerequisites, building Code Looper, and running your first loop against a real repository. If you are a UAT tester, follow it top to bottom — the [First loop walkthrough](#first-loop-walkthrough) section at the end shows exactly what a successful run looks like so you can confirm you're set up correctly before moving on to GitHub-integrated workflows.

## Prerequisites

| Requirement | Notes |
|-------------|-------|
| Rust stable | Install via [rustup.rs](https://rustup.rs) |
| Git | Used for branch operations |
| At least one provider CLI | `claude` (Claude Code), `gh copilot` (GitHub Copilot), or `codex` (OpenAI Codex) — see [Install and authenticate a provider CLI](#install-and-authenticate-a-provider-cli) |
| GitHub CLI (`gh`) | Required for GitHub-integrated workflows; install from [cli.github.com](https://cli.github.com) |
| `GITHUB_TOKEN` env var | Required when `--issue-tracking-mode=github` or `--orchestration` is set |

### Dev container (recommended)

If you use VS Code or a compatible editor, the provided dev container already has everything installed (Rust, Node, `gh`, `claude`, `gh copilot`, `uv`). Open the repo in the container and skip to [Build](#build). You will still need to authenticate the provider CLI you plan to use.

## Install and authenticate a provider CLI

Code Looper drives an existing provider CLI — it does not ship its own authentication. Before running any loop, install and log in to at least one provider:

| Provider | Install | First-run auth |
|----------|---------|----------------|
| Claude Code | `npm install -g @anthropic-ai/claude-code` | Run `claude` once; follow the browser login prompt |
| GitHub Copilot | `gh auth login` then `gh extension install github/gh-copilot` | `gh copilot` inherits `gh auth` credentials |
| OpenAI Codex | `npm install -g @openai/codex` | `codex login` |

Verify the binary is on `$PATH` before continuing:

```bash
which claude        # or: gh copilot --help  / which codex
```

If the command is missing, the loop will fail at startup with `provider process spawn failed: No such file or directory`.

## Build

```bash
git clone https://github.com/jamesbrayton/code-looper.git
cd code-looper
cargo build --release
```

The binary is written to `./target/release/code-looper`. Optionally add it to your `$PATH`:

```bash
cp target/release/code-looper ~/.local/bin/
```

The rest of this guide assumes `code-looper` is on `$PATH`. If it is not, substitute the full path to the built binary.

## Bootstrap the target repository

Code Looper does **not** run against its own source tree — it runs against a *target* repository you want to iterate on. Change to that repository first:

```bash
cd /path/to/target/repo
```

Then run `bootstrap` to create (or patch) the two files Code Looper requires:

```bash
code-looper bootstrap
```

This is idempotent and safe to run multiple times. It will:

- Create `CLAUDE.md` with a Code Looper section if no instruction file exists, or append the section to an existing `CLAUDE.md` / `AGENTS.md` / `.github/copilot-instructions.md`.
- Create `.mcp.json` with a GitHub MCP server stub (Docker-based by default) if one does not already exist, or merge the `github` entry into an existing file.

Preview what would change without writing anything:

```bash
code-looper bootstrap --dry-run
```

See [docs/workspace-prerequisites.md](workspace-prerequisites.md) for the full list of checks and remediation options.

## Run a minimal loop

The simplest invocation runs one iteration with an inline prompt and streams the provider output to your terminal:

```bash
code-looper \
  --provider claude \
  --iterations 1 \
  --prompt-inline "List the open TODO comments in this repository"
```

Key flags:

| Flag | Description |
|------|-------------|
| `--provider` | `claude`, `copilot`, or `codex` |
| `--iterations` | Positive integer, or `-1` for infinite |
| `--prompt-inline` | Prompt text (mutually exclusive with `--prompt-file`) |
| `--prompt-file` | Path to a markdown prompt file |

## Use a config file

For repeated runs, put your configuration in `looper.toml` (or `looper.yaml` / `looper.yml`) at the repository root:

```toml
provider = "claude"
iterations = 5

[orchestration]
enabled = true
repo_owner = "my-org"
repo_name  = "my-repo"

[issue_tracking]
mode        = "github"
repo_owner  = "my-org"
repo_name   = "my-repo"
comment_issue_number = 42   # issue the engine will comment on
```

Then run without flags:

```bash
code-looper --config looper.toml
```

CLI flags override config file values when both are provided.

## Connect to GitHub

Set `GITHUB_TOKEN` in your environment (or `.env` file at the repo root) before enabling GitHub-backed features:

```bash
export GITHUB_TOKEN=ghp_...
```

The token needs `repo` scope for issue and PR operations.

Run the prerequisite checker to confirm the workspace is ready:

```bash
code-looper --config looper.toml
# The checker runs automatically at startup; use --skip-prereq-check to bypass it.
```

If checks fail, the engine prints a remediation message for each failure.

## Enable orchestration

Orchestration lets the engine pick the right workflow branch automatically — PR review when open PRs exist, issue work when there are open issues, backlog discovery otherwise:

```bash
code-looper \
  --provider claude \
  --iterations 3 \
  --orchestration \
  --repo-owner my-org \
  --repo-name my-repo
```

See [docs/orchestration.md](orchestration.md) for details on workflow branch selection and PR lifecycle configuration.

## Enable PR management

To let the engine manage pull requests, set a PR mode:

```bash
# single-pr: open one PR and keep pushing to it
code-looper \
  --provider claude \
  --iterations 5 \
  --orchestration \
  --repo-owner my-org \
  --repo-name my-repo \
  --pr-mode single-pr

# multi-pr: triage open PRs every iteration before doing issue work
code-looper \
  --provider claude \
  --iterations 5 \
  --orchestration \
  --repo-owner my-org \
  --repo-name my-repo \
  --pr-mode multi-pr
```

## First loop walkthrough

This is the end-to-end path a UAT tester should run before reporting any bugs. It uses `claude` as the provider; substitute `copilot` or `codex` if you prefer, but note that `codex` currently has no MCP tool support (see [docs/providers.md](providers.md)).

1. **Pick a throwaway repository** (a forked sandbox or a scratch clone), not a repository with work you cannot afford to lose. The loop does not push or open PRs in its default mode, but it will let the agent edit files.
2. **Bootstrap it** once:
   ```bash
   cd /path/to/sandbox
   code-looper bootstrap
   git status   # expect: modified CLAUDE.md and/or new .mcp.json
   ```
3. **Commit the bootstrap output** so you have a clean baseline to diff against:
   ```bash
   git add CLAUDE.md .mcp.json
   git commit -m "Add Code Looper bootstrap"
   ```
4. **Run a single iteration** with a small, verifiable prompt:
   ```bash
   code-looper \
     --provider claude \
     --iterations 1 \
     --prompt-inline "List every Cargo.toml or package.json file in this repo and print their top-level names"
   ```
5. **Confirm what you should see** while the loop runs:
   - Startup logs from the prerequisite checker (`✓ instruction-file`, `✓ instruction-section`, `✓ mcp-github-server`).
   - The provider's streamed stdout (agent reasoning + tool calls).
   - A structured `Iteration complete` log line with `outcome=success` and non-zero `duration_ms` once the iteration finishes.
   - An end-of-run summary printed to the terminal and written to `summary.md`.
6. **Inspect the artifacts** written under `.code-looper/runs/<run-id>/`:
   ```
   .code-looper/runs/<run-id>/
   ├── iteration-1.log   # per-iteration transcript
   ├── manifest.json     # structured run metadata
   └── summary.md        # human-readable summary
   ```
   `iteration-1.log` is written incrementally, so it is present even if the run is interrupted. `manifest.json` and `summary.md` are only written at clean exit.
7. **Verify the sandbox is still clean** — `git status` should show no unintended changes unless the prompt asked the agent to edit files.

If any of steps 2–7 fail, capture the output and check [docs/troubleshooting.md](troubleshooting.md) before reporting a bug. The most common first-run issues are provider CLI not on `$PATH` and `.mcp.json` missing the `github` entry (both caught by the startup checks).

## Next steps

- [docs/configuration.md](configuration.md) — Every config field, CLI flag, default, and precedence rule
- [docs/providers.md](providers.md) — Provider adapter invocation, environment requirements, and known limitations
- [docs/orchestration.md](orchestration.md) — Workflow branches, shippable signal, PR lifecycle, multi-PR triage
- [docs/workspace-prerequisites.md](workspace-prerequisites.md) — What the prerequisite checker validates and how to fix each diagnostic
- [docs/troubleshooting.md](troubleshooting.md) — Common failure modes and remediation steps
- [docs/PRD.md](PRD.md) — Full product requirements and roadmap
- [CLAUDE.md](../CLAUDE.md) — Contributor workflow and issue discipline
