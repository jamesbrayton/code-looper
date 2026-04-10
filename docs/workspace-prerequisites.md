# Workspace Prerequisites

Before starting the loop, Code Looper validates that the workspace satisfies a set of prerequisites. This ensures the agent has the configuration it needs and that GitHub operations are routed through the MCP server.

## Running the check

The prerequisite check runs automatically at startup unless disabled. To skip it:

```bash
code-looper --skip-prereq-check ...
```

Use `--skip-prereq-check` only when the workspace is not a git repository or the checks genuinely do not apply (for example, in CI environments where all prerequisites are known to be satisfied).

## What is checked

### Check: `instruction-file`

**What it looks for**

The checker looks for at least one of the following files in the workspace root:

- `CLAUDE.md`
- `AGENTS.md`
- `.github/copilot-instructions.md`

These files contain project-specific instructions for the agent. Without them, the agent will run without any project context, which typically produces generic or unhelpful results.

**Failure message**

```
[instruction-file] No instruction file found in '<dir>'. Expected one of: CLAUDE.md, AGENTS.md, .github/copilot-instructions.md
  → Remediation: Create a CLAUDE.md (or AGENTS.md) at the repository root with agent instructions for this project.
```

**Remediation**

Create `CLAUDE.md` at the repository root. At a minimum, include:
- What the project does
- How to build and test it
- Any conventions the agent should follow (commit style, branch naming, etc.)

Example:
~~~markdown
# CLAUDE.md

## Project

A Rust CLI that does X.

## Build

```bash
cargo build
cargo test
```

## Conventions

- Commit messages: `<type>: <summary> (#<issue>)`
~~~

---

### Check: `instruction-section`

**What it looks for**

Once an instruction file is found, the checker verifies that it contains the Code Looper section begin marker:

```
<!-- code-looper begin -->
```

This marker delimits the block that `code-looper bootstrap` manages. Its presence confirms the file has been configured for Code Looper use.

**Failure message**

```
[instruction-section] '<file>' does not contain the Code Looper section (expected marker: `<!-- code-looper begin -->`).
  → Remediation: Run `code-looper bootstrap` to inject the required Code Looper section into the instruction file.
```

Note: this check is skipped when `instruction-file` also fails — it only runs when an instruction file was found.

**Remediation**

Run `code-looper bootstrap` to append the Code Looper section automatically, or add the markers manually:

```markdown
<!-- code-looper begin -->
## Code Looper

... Code Looper configuration here ...
<!-- code-looper end -->
```

---

### Check: `mcp-github-server`

**What it looks for**

The checker looks for `.mcp.json` in the workspace root and verifies that it contains a `"github"` key, indicating that the GitHub MCP server is configured.

**Why this matters**

Code Looper enforces MCP-only GitHub mutation by default. If the GitHub MCP server is not configured, orchestration flows that need to read PR/issue state or post comments will fail at runtime. The startup check surfaces this early with actionable guidance rather than failing mid-run.

**Failure messages**

```
[mcp-github-server] No .mcp.json found in '<dir>'
  → Remediation: Create a .mcp.json that includes a "github" MCP server entry. See https://docs.anthropic.com/en/docs/claude-code/mcp for details.
```

```
[mcp-github-server] <file> does not contain a "github" MCP server entry
  → Remediation: Add a GitHub MCP server block under the "mcpServers" key in .mcp.json so that orchestration flows can use GitHub tools.
```

**Remediation**

Create `.mcp.json` at the repository root with at minimum a `github` entry.
Either of the two common stubs below works — pick whichever matches your
local tooling.  Note that `code-looper bootstrap` writes the Docker-based
variant by default (see `MCP_STUB` in `src/bootstrap.rs`); the `npx` form
is shown here for reference because it is the more portable option when
Docker is not available.

```json
{
  "mcpServers": {
    "github": {
      "command": "docker",
      "args": [
        "run", "-i", "--rm",
        "-e", "GITHUB_PERSONAL_ACCESS_TOKEN",
        "ghcr.io/github/github-mcp-server"
      ],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "${GITHUB_TOKEN}"
      }
    }
  }
}
```

Or, using `npx`:

```json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "${GITHUB_TOKEN}"
      }
    }
  }
}
```

Ensure `GITHUB_TOKEN` is set in your environment (or in a `.env` file). The token needs at minimum `repo` scope for read operations, and `repo` + `issues` + `pull_requests` for write operations.

---

## Bypassing the MCP-only constraint

The `--allow-direct-github` flag disables the MCP-only enforcement. This means:

- The agent is not prompted to use MCP tools for GitHub mutations.
- The orchestration engine may use direct `gh` CLI calls for context resolution.

**This flag is unsafe for production use.** It exists primarily for local development and provider adapters (like `codex`) that lack MCP tool support. When set, direct `gh` mutations bypass the audit trail and policy guard layer.

---

## Workspace directory

By default, prerequisite checks run against the current working directory. Override this with:

```bash
code-looper --workspace-dir /path/to/repo ...
```

This is useful when Code Looper is invoked from a directory other than the repository root.

---

## Bootstrap subcommand

Instead of manually creating the required files, use the `bootstrap` subcommand
to let Code Looper create or patch them automatically:

```bash
code-looper bootstrap
```

Bootstrap is idempotent — running it on an already-configured repository
produces no changes and exits 0.

### What bootstrap does

| Prerequisite | Action |
|---|---|
| No instruction file found | Creates `CLAUDE.md` with a Code Looper section |
| Instruction file lacks a Code Looper section | Appends a delimited block |
| `.mcp.json` missing | Creates a minimal stub with the GitHub server entry |
| `.mcp.json` lacks a `"github"` key | Merges the entry into the existing file |

### Dry-run mode

Preview changes without writing anything:

```bash
code-looper bootstrap --dry-run
```

### Custom workspace directory

```bash
code-looper bootstrap --workspace-dir /path/to/repo
```

### Safety guarantees

- Existing content is never removed or overwritten.
- The Code Looper section in instruction files is delimited by
  `<!-- code-looper begin -->` and `<!-- code-looper end -->` markers, making
  it easy to identify and remove if needed.
- `.mcp.json` keys outside the `github` entry are not modified.
