# Code Looper

Code Looper is a Rust CLI loop engine that drives multiple coding-agent CLIs (Claude Code, GitHub Copilot CLI, Codex CLI) through configurable iterations with policy-driven orchestration. Instead of writing one-off shell scripts per project, you configure a loop once and let Code Looper manage iteration count, retry behavior, provider selection, PR lifecycle, and issue tracking.

## Status

**Early / experimental.** Milestones M1–M14 are complete (core engine, all three provider adapters, orchestration policy, issue tracking, branch and PR lifecycle, multi-PR triage). The project follows the roadmap in [docs/PRD.md](docs/PRD.md). Breaking changes are possible until v1.0.

## Quick start

Prerequisites: Rust toolchain (`rustup`), and at least one provider CLI on your `$PATH` (`claude`, `gh copilot`, or `codex`).

```bash
# Clone and build
git clone https://github.com/jamesbrayton/code-looper.git
cd code-looper
cargo build --release

# Run a single iteration with an inline prompt
./target/release/code-looper \
  --provider claude \
  --iterations 1 \
  --prompt-inline "Describe the repository structure"
```

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

**Environment variables:** Copy `.env.example` to `.env` and set `GITHUB_TOKEN` to a personal access token with `repo` scope. The GitHub MCP server reads this token at startup.

**MCP servers:** Configured in `.mcp.json`:

| Server | Purpose |
|--------|---------|
| `github` | GitHub API for issue/PR read and write |
| `context7` | Library documentation lookup |
| `markitdown` | Document format conversion |
| `microsoftdocs` | Microsoft Learn / Azure documentation |

## Documentation

| Document | Description |
|----------|-------------|
| [docs/getting-started.md](docs/getting-started.md) | Install, configure, and run your first loop |
| [docs/orchestration.md](docs/orchestration.md) | Workflow branch selection, shippable signal protocol, PR lifecycle |
| [docs/PRD.md](docs/PRD.md) | Full product requirements and roadmap |

Additional reference docs (configuration, providers, workspace prerequisites, troubleshooting) are tracked in [#9](https://github.com/jamesbrayton/code-looper/issues/9).

## Contributing

Every change must be tracked by a GitHub issue before any code is written. See [CLAUDE.md](CLAUDE.md) for the full contributor workflow — issue creation, commit message format, comment cadence, and handoff expectations.

```bash
# Reference the issue in every commit
git commit -m "Add retry backoff configuration (#42)"
```

## License

[MIT](LICENSE)
