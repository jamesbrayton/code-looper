---
name: github-issues
description: 'Create, update, and manage GitHub issues using MCP tools. Use this skill when users want bug reports, feature requests, task tracking, issue comments, issue-type selection, or issue workflow updates. Triggers on requests like "create an issue", "file a bug", "request a feature", "update issue X", "comment on issue", or any GitHub issue management task.'
---

# GitHub Issues

Manage GitHub issues using the GitHub MCP server.

## Available MCP Tools

| Tool | Purpose |
|------|---------|
| `issue_write` | Create or update an issue |
| `issue_read` | Read issue details, comments, labels, and sub-issues |
| `add_issue_comment` | Add progress, decisions, and handoff notes |
| `list_issues` | List issues in a repository |
| `search_issues` | Search issues by query syntax |
| `list_issue_types` | List available issue types for an owner/org |
| `sub_issue_write` | Add/remove/reprioritize sub-issues |

## Workflow

1. **Determine action**: Create, update, query, or comment?
2. **Determine issue type**: Use issue `type` (`bug`, `feature`, `task`) instead of type-like labels.
3. **Gather context**: Confirm owner/repo, fetch issue (if updating), and call `list_issue_types` when type availability is unknown.
4. **Structure content**: Use the correct template file from `references/`.
5. **Execute**: Call the appropriate MCP tool with only the fields being changed.
6. **Maintain execution log**: Add milestone comments and keep issue body sections/checklists current.
7. **Confirm**: Report the resulting issue URL and what was changed.

## Creating Issues

### Required Parameters

```
owner: repository owner (org or user)
repo: repository name  
title: clear, actionable title
body: structured markdown content
type: issue type (prefer bug, feature, or task when available)
```

### Optional Parameters

```
labels: ["documentation", "good first issue", "help wanted", ...]
assignees: ["username1", "username2"]
milestone: milestone number (integer)
```

Do not use labels like `bug`, `feature`, or `task` as substitutes for issue type. Set those via the `type` field.

### Title Guidelines

- Start with type prefix when useful: `[Bug]`, `[Feature]`, `[Docs]`
- Be specific and actionable
- Keep under 72 characters
- Examples:
  - `[Bug] Login fails with SSO enabled`
  - `[Feature] Add dark mode support`
  - `Add unit tests for auth module`

### Body Structure

Always use the templates in `references/` and choose based on request intent:

| User Request | Template |
|--------------|----------|
| Bug, error, broken, not working | [references/bug_report.md](references/bug_report.md) |
| Feature, enhancement, add, new | [references/feature_request.md](references/feature_request.md) |
| Task, chore, refactor, update | [references/task.md](references/task.md) |

## Updating Issues

Use `issue_write` with `method: "update"`:

```
owner, repo, issue_number (required)
title, body, state, type, labels, assignees, milestone (optional - only changed fields)
```

State values: `open`, `closed`

## Examples

### Example 1: Bug Report

**User**: "Create a bug issue - the login page crashes when using SSO"

**Action**: Call `issue_write` with:
```json
{
  "method": "create",
  "owner": "github",
  "repo": "awesome-copilot",
  "title": "[Bug] Login page crashes when using SSO",
  "type": "bug",
  "body": "## Description\nThe login page crashes when users attempt to authenticate using SSO.\n\n## Steps to Reproduce\n1. Navigate to login page\n2. Click 'Sign in with SSO'\n3. Page crashes\n\n## Expected Behavior\nSSO authentication should complete and redirect to dashboard.\n\n## Actual Behavior\nPage becomes unresponsive and displays error.\n\n## Environment\n- Browser: [To be filled]\n- OS: [To be filled]\n\n## Additional Context\nReported by user.",
  "labels": ["high-priority"]
}
```

### Example 2: Feature Request

**User**: "Create a feature request for dark mode with high priority"

**Action**: Call `issue_write` with:
```json
{
  "method": "create",
  "owner": "github",
  "repo": "awesome-copilot",
  "title": "[Feature] Add dark mode support",
  "type": "feature",
  "body": "## Summary\nAdd dark mode theme option for improved user experience and accessibility.\n\n## Motivation\n- Reduces eye strain in low-light environments\n- Increasingly expected by users\n- Improves accessibility\n\n## Proposed Solution\nImplement theme toggle with system preference detection.\n\n## Acceptance Criteria\n- [ ] Toggle switch in settings\n- [ ] Persists user preference\n- [ ] Respects system preference by default\n- [ ] All UI components support both themes\n\n## Alternatives Considered\nNone specified.\n\n## Additional Context\nHigh priority request.",
  "labels": ["high-priority"]
}
```

## Type And Label Guidance

Use issue `type` for classification and labels for orthogonal metadata.

### Recommended Issue Types

| Type | Use For |
|------|---------|
| `bug` | Something is broken or behaving incorrectly |
| `feature` | New capability or enhancement |
| `task` | Planned unit of implementation work |

### Useful Labels (Non-Type)

| Label | Use For |
|-------|---------|
| `documentation` | Documentation updates |
| `good first issue` | Good for newcomers |
| `help wanted` | Extra attention needed |
| `question` | Further information requested |
| `wontfix` | Will not be addressed |
| `duplicate` | Already exists |
| `high-priority` | Urgent issues |

Avoid using `bug`, `feature`, or `task` as labels when those are available as issue types.

## Issue Comments And Iteration Logging

Issue comments are the canonical execution log while work is in progress.

- Add comments at meaningful milestones, such as:
  - Scope clarified and implementation plan finalized
  - First implementation pass complete
  - Tests added/updated and results captured
  - Blocker discovered or dependency identified
  - Handoff to another agent/person
- In the same update cycle, keep the issue body current:
  - Update checklist progress
  - Update notes/decisions sections
  - Add or revise blockers/dependencies
- Ensure handoff quality:
  - A new contributor should be able to read the issue and resume work immediately
  - Include what changed, what is left, and where to continue
  - Reference repo state when relevant (branch, PR, commit, failing test, artifact)

### Comment Content Checklist

- Current milestone reached
- Concrete changes made
- Validation performed (tests/checks)
- Remaining work
- Blockers or decisions needing input

## Tips

- Always confirm the repository context before creating issues
- Ask for missing critical information rather than guessing
- Link related issues when known: `Related to #123`
- For updates, fetch current issue first to preserve unchanged fields
- If issue types are uncertain for a repo/org, call `list_issue_types` before `issue_write`
