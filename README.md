# Code Looper

Code Looper is a Rust CLI loop engine that drives multiple coding-agent CLIs (Claude Code, GitHub Copilot CLI, Codex CLI) through configurable iterations with policy-driven orchestration. Instead of writing one-off shell scripts per project, you configure a loop once and let Code Looper manage iteration count, retry behavior, provider selection, PR lifecycle, and issue tracking.

## Status

**Early / experimental — ready for first-round user acceptance testing.** Milestones M1–M14 are complete (core engine, all three provider adapters, orchestration policy, issue tracking, branch and PR lifecycle, multi-PR triage). The project follows the roadmap in [docs/PRD.md](docs/PRD.md). Breaking changes are possible until v1.0.

If you are a UAT tester, start with [docs/getting-started.md](docs/getting-started.md) — it walks through install, first run, and what successful output looks like end-to-end.

## Quick Install

```sh
curl -fsSL https://raw.githubusercontent.com/jamesbrayton/code-looper/main/install.sh | sh
```

Detects your OS and architecture, downloads the correct binary from [GitHub Releases](https://github.com/jamesbrayton/code-looper/releases/latest), and installs to `~/.local/bin`. Override with `curl -fsSL https://raw.githubusercontent.com/jamesbrayton/code-looper/main/install.sh | INSTALL_DIR=/custom/path sh -s --`.

**Supported platforms:** macOS ARM64 (Apple Silicon), Linux x86\_64, Linux ARM64.

**macOS x86\_64 (Intel):** No pre-built binary — build from source (see below).

**Windows:** Download `code-looper-<version>-x86_64-pc-windows-msvc.zip` from the [latest release](https://github.com/jamesbrayton/code-looper/releases/latest), extract it, rename the extracted `code-looper-<version>-x86_64-pc-windows-msvc.exe` to `code-looper.exe`, and move it to a directory on your `PATH`.

## Quick start

Prerequisites:

- **Rust toolchain** (install via [rustup.rs](https://rustup.rs))
- **At least one provider CLI on `$PATH`**, *already authenticated*:
  - `claude` — install with `npm install -g @anthropic-ai/claude-code`, then run `claude` once to log in
  - `gh copilot` — install with `gh extension install github/gh-copilot` (requires `gh auth login` first)
  - `codex` — install with `npm install -g @openai/codex`, then run `codex login`

```bash
# 1. Clone and build
git clone https://github.com/jamesbrayton/code-looper.git
cd code-looper
cargo build --release

# 2. In the target repository you want to loop over, set up the workspace
#    prerequisites (creates CLAUDE.md section + .mcp.json stub as needed).
cd /path/to/target/repo
/path/to/code-looper/target/release/code-looper bootstrap

# 3. Run a single iteration with an inline prompt.
/path/to/code-looper/target/release/code-looper \
  --provider claude \
  --iterations 1 \
  --prompt-inline "Describe the repository structure"
```

The run writes artifacts under `.code-looper/runs/<run-id>/` (per-iteration logs, manifest, and summary).

For a GitHub-integrated workflow with issue tracking and PR management, see [docs/getting-started.md](docs/getting-started.md).

## Build & test

```bash
cargo build              # debug build
cargo build --release    # release build
cargo test               # run all tests
cargo test <test_name>   # run a single test
cargo clippy             # lint
cargo fmt                # format code
cargo fmt -- --check     # check formatting without modifying
```

The test suite is pure unit tests and runs without any provider CLI installed.

## Development environment

A fully configured dev container is provided (`.devcontainer/`). It is based on `mcr.microsoft.com/devcontainers/rust:2-1-trixie` and pre-installs:

- Rust toolchain (stable)
- Node.js LTS
- GitHub CLI (`gh`)
- Claude Code CLI
- GitHub Copilot CLI
- `uv` (Python package manager)

**Environment variables:** Copy `.env.example` to `.env` and set `GITHUB_TOKEN` to a personal access token with `repo` scope. The `gh` CLI uses this token for GitHub operations; it is also consumed by the GitHub MCP server when MCP fallback is in use.

**MCP servers:** Configured in `.mcp.json`. The `github` entry is optional in the default (`gh`-first) mode and only required when `allow_direct_github = false` (strict MCP-only mode):

| Server | Purpose |
|--------|---------|
| `github` | GitHub API fallback when `gh` is unavailable (optional in default mode) |
| `context7` | Library documentation lookup |
| `markitdown` | Document format conversion |
| `microsoftdocs` | Microsoft Learn / Azure documentation |

## Documentation

| Document | Description |
|----------|-------------|
| [docs/getting-started.md](docs/getting-started.md) | Install, configure, and run your first loop |
| [docs/configuration.md](docs/configuration.md) | Every config field, CLI flag, default, and precedence rule |
| [docs/providers.md](docs/providers.md) | Provider adapter invocation, environment requirements, and limitations |
| [docs/orchestration.md](docs/orchestration.md) | Workflow branch selection, shippable signal protocol, PR lifecycle |
| [docs/workspace-prerequisites.md](docs/workspace-prerequisites.md) | What the prerequisite checker validates and how to fix each diagnostic |
| [docs/troubleshooting.md](docs/troubleshooting.md) | Common failure modes and remediation steps |
| [docs/PRD.md](docs/PRD.md) | Full product requirements and roadmap |

## Contributing

Every change must be tracked by a GitHub issue before any code is written. See [CLAUDE.md](CLAUDE.md) for the full contributor workflow — issue creation, commit message format, comment cadence, and handoff expectations.

```bash
# Reference the issue in every commit
git commit -m "Add retry backoff configuration (#42)"
```

## License

[MIT](LICENSE)
