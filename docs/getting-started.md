# Getting Started with Code Looper

This guide walks you through installing prerequisites, building Code Looper, and running your first loop against a real repository.

## Prerequisites

| Requirement | Notes |
|-------------|-------|
| Rust stable | Install via [rustup.rs](https://rustup.rs) |
| Git | Used for branch operations |
| At least one provider CLI | `claude` (Claude Code), `gh copilot` (GitHub Copilot), or `codex` (OpenAI Codex) |
| GitHub CLI (`gh`) | Required for GitHub-integrated workflows; install from [cli.github.com](https://cli.github.com) |
| `GITHUB_TOKEN` env var | Required when `--issue-tracking-mode=github` or `--orchestration` is set |

### Dev container (recommended)

If you use VS Code or a compatible editor, the provided dev container already has everything installed. Open the repo in the container and skip to [Build](#build).

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

## Next steps

- [docs/configuration.md](configuration.md) — Every config field, CLI flag, default, and precedence rule
- [docs/providers.md](providers.md) — Provider adapter invocation, environment requirements, and known limitations
- [docs/orchestration.md](orchestration.md) — Workflow branches, shippable signal, PR lifecycle, multi-PR triage
- [docs/workspace-prerequisites.md](workspace-prerequisites.md) — What the prerequisite checker validates and how to fix each diagnostic
- [docs/troubleshooting.md](troubleshooting.md) — Common failure modes and remediation steps
- [docs/PRD.md](PRD.md) — Full product requirements and roadmap
- [CLAUDE.md](../CLAUDE.md) — Contributor workflow and issue discipline
