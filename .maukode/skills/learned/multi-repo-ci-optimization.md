---
name: multi-repo-ci-optimization
description: "Audit and optimize CI workflows across many separate repos as a batch operation"
run_as_subagent: false
---
To optimize CI across multiple repos: (1) Enumerate all repos and their `.github/workflows/*.yml` files. (2) Build a gap analysis matrix showing which repos lack sccache, path filters, concurrency/cancel-in-progress, rust-cache, and mold linker. (3) Apply changes repo-by-repo using bash (write_file may be blocked outside workspace jail). (4) Validate all YAML before committing. (5) Use `git add <specific-file>` to avoid bundling unrelated unstaged changes. (6) Handle branch name differences (`main` vs `master`) when rebasing/pushing. (7) Use full absolute paths for any custom binaries in env vars. (8) Push to existing feature branches, then verify CI runs via `gh run list` and `gh run view`. (9) Check sccache stats after first run to confirm cache is working (expect low hits on first run).
