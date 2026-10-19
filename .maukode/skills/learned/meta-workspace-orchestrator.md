---
name: meta-workspace-orchestrator
description: "Create a bash-based orchestrator script to manage multiple separate git repos as a single unified project with status, CI, branches, and dependency visibility."
run_as_subagent: false
---
When a user has many separate repos that form one project, create a `.project/rk.sh` orchestrator script that: (1) reads a `repos.toml` listing all repos with their dependency layers, (2) provides subcommands: `info` (overview counts), `status` (git dirty/ahead/behind across all repos), `ci` (GitHub Actions status via `gh run list`), `branches` (non-main branches highlighted), `worktrees` (git worktree mapping), and `deps` (dependency layer graph). Use ANSI colors for visual clarity. Add a shell alias pointing to the script. Store the config in a dot-directory at the parent workspace level, not inside any individual repo.
