---
name: multi-repo-ci-optimizer
description: "Batch-optimize GitHub Actions CI workflows across multiple repositories with sccache, path filters, concurrency, and caching."
run_as_subagent: false
---
Given a set of repos in a workspace directory, for each repo's `.github/workflows/ci.yml`: (1) Add sccache env vars with absolute binary path and shared cache dir, plus start-server and show-stats steps; (2) Add `concurrency` with `cancel-in-progress: true`; (3) Add `paths` filters to skip CI on non-source changes (src/**, crates/**, tests/**, scripts/**, Cargo.toml, Cargo.lock, .github/workflows/**); (4) Add `Swatinem/rust-cache@v2` if missing; (5) Add mold linker + clang config step for Rust repos. Validate YAML after each change, commit only the ci.yml file (use `git add .github/workflows/ci.yml` specifically to avoid staging unrelated dirty files), push to current branch, and verify CI runs succeed. When rebasing across repos with different default branch names (main vs master), resolve conflicts by preserving the repo's existing branch name in `branches:` and applying only the new optimizations.
