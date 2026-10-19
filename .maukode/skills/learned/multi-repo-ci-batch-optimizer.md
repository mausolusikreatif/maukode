---
name: multi-repo-ci-batch-optimizer
description: "Systematically optimize GitHub Actions CI workflows across many repos with sccache, path filters, concurrency, and rust-cache"
run_as_subagent: false
---
When optimizing CI across multiple repos: (1) Audit all existing workflows for gaps (missing sccache, concurrency, path filters, rust-cache, mold linker). (2) Install sccache on the self-hosted runner and symlink to all runner homes. (3) Patch each workflow via bash (not write_file, which is workspace-jailed) using sed/cat heredocs. (4) Use RUSTC_WRAPPER with absolute path to sccache binary. (5) Validate all YAML before committing. (6) Commit only the CI file using `git reset HEAD --` to exclude unrelated unstaged changes. (7) Push and monitor CI runs, fixing failures iteratively. (8) Cherry-pick CI commits to main on all repos to keep local branches consistent.
