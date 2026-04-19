# ADR-009: Documentation Platform and Structure

**Date:** 2026-04-19
**Status:** Accepted
**Deciders:** James Brayton

## Context

Code Looper's documentation lives in `docs/` as raw Markdown files browsable via GitHub's file
viewer. With a growing number of pages (configuration, providers, orchestration, workspace
prerequisites, troubleshooting, prompt injection, PRD, and eight ADRs), a structured, searchable,
navigable documentation site is needed.

Three static site generators were evaluated:

- **MkDocs + Material** — Python-based, excellent navigation and search. MkDocs v2.0 introduces
  breaking changes for the Material theme, creating migration uncertainty. Ruled out.
- **Jekyll** — GitHub's default; no build step required, but limited layout control and no
  Rust ecosystem fit. Ruled out.
- **mdBook** — Rust-native, used by the official Rust Book and many Rust projects. Zero Python
  dependency, clean professional output, trivial GitHub Actions integration. Selected.

## Decision

Use **mdBook** as the documentation platform and publish to GitHub Pages via GitHub Actions.

Organize content using the **Diataxis framework** — four quadrants (Tutorials, How-to guides,
Reference, Explanation) plus a **"Design & Architecture"** section for the PRD and ADRs.

Deployment trigger: `on: push: branches: [main]` — fires on every PR merge to main.

## Rationale

- MkDocs was ruled out due to the v2.0/Material breaking-changes concern, not a fundamental
  objection to MkDocs itself. If Material stabilizes on v2.0 this decision could be revisited.
- mdBook is the natural fit for a Rust CLI project and eliminates a Python dependency from CI.
- Diataxis provides a principled structure for organizing existing content without requiring new
  content to be written. The Tutorials quadrant is intentionally left empty at launch, with a
  follow-up issue filed.
- A "Design & Architecture" section exposes the PRD and ADRs to curious consumers without
  mixing them into the user-facing navigation.

## Consequences

- `docs/` is reorganized: `docs/src/` becomes the mdBook source directory with Diataxis
  subdirectories. Internal artifacts (`docs/superpowers/`) remain in `docs/` but outside
  `docs/src/` and are not published.
- `book.toml` is added to the repository root.
- All internal cross-links between documentation pages must be updated to reflect new paths.
- A GitHub Actions workflow (`.github/workflows/docs.yml`) builds and deploys on merge to main.
- GitHub Pages must be enabled in repository settings with "GitHub Actions" as the source
  (one-time manual step).
- The Tutorials quadrant has no content at launch; a follow-up issue tracks content creation.
