# Ralph Loop

Iterative autonomous development script that runs Claude CLI in a loop, driving the codebase toward the end-state described in `docs/PRD.md`.

Each iteration is an independent Claude session that examines the current repo state, picks the highest-impact unblocked work, and makes committable progress — creating and updating GitHub issues along the way.

## Usage

```bash
# From the repo root
.claude/ralph-loop.sh [iterations] [model]
```

| Argument | Default | Description |
|----------|---------|-------------|
| `iterations` | `5` | Number of loop iterations to run |
| `model` | `opus` | Claude model to use (`opus`, `sonnet`, `haiku`, or a full model ID) |

### Examples

```bash
# Run 5 iterations with opus (defaults)
.claude/ralph-loop.sh

# Run 10 iterations
.claude/ralph-loop.sh 10

# Run 3 iterations with sonnet (faster, cheaper)
.claude/ralph-loop.sh 3 sonnet

# Run 1 iteration to test the loop
.claude/ralph-loop.sh 1
```

## How It Works

1. **Fresh session per iteration.** Each iteration launches a new `claude --print` session. This ensures Claude re-examines the current repo state (including changes from prior iterations) rather than accumulating stale context.

2. **PRD-driven prioritization.** The script injects instructions telling Claude to follow the PRD's phased rollout (Phase 1 milestones M1 through M6), working on the earliest unfinished milestone.

3. **GitHub issue tracking.** Every iteration checks for open issues, creates new ones as needed, references them in commits, and comments with progress updates. This provides a complete audit trail across iterations.

4. **Autonomous permissions.** The script runs with `--permission-mode auto`, allowing Claude to read, write, execute, and commit without interactive prompts.

## Logs

Session and iteration logs are written to `.claude/ralph-loop-logs/` (gitignored).

```
.claude/ralph-loop-logs/
├── session-20260317-143022.log        # Summary log for the full run
├── iteration-20260317-143022-1.log    # Full output from iteration 1
├── iteration-20260317-143022-2.log    # Full output from iteration 2
└── ...
```

- **Session log**: Timestamps, iteration counts, and exit codes for the overall run.
- **Iteration logs**: Complete Claude CLI output for each iteration, useful for debugging or reviewing what was done.

## Prerequisites

- `claude` CLI installed and authenticated (`claude auth`)
- `GITHUB_TOKEN` set in `.env` (for issue creation/updates via MCP)
- `docs/PRD.md` must exist (the script exits with an error if missing)

## Tips

- **Start small.** Run 1-2 iterations first to verify the loop is producing the kind of changes you want before running longer sessions.
- **Review between runs.** Check `git log` and open GitHub issues after a run to understand what was accomplished before starting the next.
- **Use sonnet for scaffolding.** Early iterations (project setup, boilerplate) don't need opus. Switch to opus for complex agent logic.
- **Monitor costs.** Each iteration is a full Claude session. Longer runs with opus will consume more API credits.
