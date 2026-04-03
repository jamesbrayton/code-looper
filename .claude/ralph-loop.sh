#!/usr/bin/env bash
set -euo pipefail

# Ralph Loop - Iterative autonomous development driven by the PRD
#
# Runs Claude CLI in a loop, each iteration examining the current repo state
# and making progress toward the end-state described in docs/PRD.md.
#
# Usage:
#   .claude/ralph-loop.sh              # Default: 5 iterations
#   .claude/ralph-loop.sh 10           # Run 10 iterations
#   .claude/ralph-loop.sh 3 sonnet     # Run 3 iterations with sonnet model

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ITERATIONS="${1:-5}"
MODEL="${2:-opus}"
PRD_PATH="${REPO_ROOT}/docs/PRD.md"
LOG_DIR="${REPO_ROOT}/.claude/ralph-loop-logs"

if [[ ! -f "$PRD_PATH" ]]; then
    echo "ERROR: PRD not found at ${PRD_PATH}"
    exit 1
fi

mkdir -p "$LOG_DIR"

TIMESTAMP=$(date +%Y%m%d-%H%M%S)
SESSION_LOG="${LOG_DIR}/session-${TIMESTAMP}.log"

echo "=== Ralph Loop ===" | tee "$SESSION_LOG"
echo "Iterations: ${ITERATIONS}" | tee -a "$SESSION_LOG"
echo "Model: ${MODEL}" | tee -a "$SESSION_LOG"
echo "PRD: ${PRD_PATH}" | tee -a "$SESSION_LOG"
echo "Log: ${SESSION_LOG}" | tee -a "$SESSION_LOG"
echo "Started: $(date)" | tee -a "$SESSION_LOG"
echo "==================" | tee -a "$SESSION_LOG"

read -r -d '' PROMPT <<'PROMPT_EOF' || true
You are operating in Ralph Loop mode — an iterative autonomous development loop.

## Your Mission

Read the PRD at docs/PRD.md. This is the end-state for this solution. Your job is to make meaningful progress toward that end-state in this single iteration.

## Rules

1. **Examine current state first.** Read CLAUDE.md, review the repo structure, check git log, and understand what already exists before making any changes.
2. **Every change needs a GitHub issue.** Check if a relevant issue exists. If not, create one. Reference the issue in commits. Comment on the issue with what you accomplished.
3. **Pick the highest-impact work** that is unblocked and not already in progress. Look at open GitHub issues to avoid duplicating work from a previous iteration.
4. **Make small, complete, committable changes.** Each iteration should produce working code that builds and passes any existing tests. Do not leave things half-done.
5. **Commit your work** with a clear message referencing the GitHub issue number.
6. **End with a status comment** on the GitHub issue summarizing what was done and what remains.

## Prioritization

When deciding what to work on, follow the PRD's phased rollout (Phase 1 milestones M1-M6). Work on the earliest unfinished milestone. Within a milestone, prioritize:
- Project scaffolding and build configuration (if not yet done)
- Core data models and interfaces
- Agent implementation
- Integration and wiring
- Tests

## Constraints

- Do not modify the PRD itself.
- Do not modify CLAUDE.md unless adding build/test commands as they become available.
- Do not skip tests — if you write code, write at least basic tests.
- Prefer simple, working implementations over elaborate stubs.
PROMPT_EOF

for ((i = 1; i <= ITERATIONS; i++)); do
    echo "" | tee -a "$SESSION_LOG"
    echo "--- Iteration ${i}/${ITERATIONS} — $(date) ---" | tee -a "$SESSION_LOG"

    ITERATION_LOG="${LOG_DIR}/iteration-${TIMESTAMP}-${i}.log"

    claude \
        --print \
        --model "$MODEL" \
        --dangerously-skip-permissions \
        --append-system-prompt "$PROMPT" \
        "Ralph Loop iteration ${i} of ${ITERATIONS}. Examine the current repo state and make progress toward the PRD end-state. Check open GitHub issues for context on prior iterations." \
        2>&1 | tee "$ITERATION_LOG"

    EXIT_CODE=${PIPESTATUS[0]}

    if [[ $EXIT_CODE -ne 0 ]]; then
        echo "WARNING: Iteration ${i} exited with code ${EXIT_CODE}" | tee -a "$SESSION_LOG"
    fi

    echo "Iteration ${i} complete (exit: ${EXIT_CODE})" | tee -a "$SESSION_LOG"
done

echo "" | tee -a "$SESSION_LOG"
echo "=== Ralph Loop Complete ===" | tee -a "$SESSION_LOG"
echo "Finished: $(date)" | tee -a "$SESSION_LOG"
echo "Total iterations: ${ITERATIONS}" | tee -a "$SESSION_LOG"
echo "Session log: ${SESSION_LOG}" | tee -a "$SESSION_LOG"
