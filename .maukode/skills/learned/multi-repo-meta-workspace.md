---
name: multi-repo-meta-workspace
description: "Create a bash orchestrator script that manages many separate git repos as one unified project with status, CI, branches, deps, and worktree commands."
run_as_subagent: false
---
Given a workspace directory containing multiple related git repositories, generate a bash script (e.g. `rk.sh`) that: (1) tracks all repos in a repos.toml manifest, (2) provides `info` (overview counts), `status` (git status across all repos with dirty/ahead/behind), `ci` (GitHub Actions status via `gh run list`), `branches` (active branch per repo), `deps` (dependency graph by parsing Cargo.toml cross-deps), and `worktrees` (list git worktrees). Use color-coded output, handle repos that aren't cloned yet, and add a shell alias for the script.
