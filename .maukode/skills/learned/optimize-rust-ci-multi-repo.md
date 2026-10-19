---
name: optimize-rust-ci-multi-repo
description: "Apply 5-layer CI optimization (sccache, concurrency, path filtering, rust-cache, mold linker) across multiple Rust repos systematically."
run_as_subagent: false
---
When optimizing CI for a multi-repo Rust project on self-hosted runners: (1) Install sccache as a prebuilt binary in a shared location and symlink to all runner homes; create a shared cache dir. (2) Add `concurrency: { group: ${{ github.workflow }}-${{ github.ref }}, cancel-in-progress: true }` to every workflow. (3) Add path filters to skip CI when only non-source files change (docs, README, .gitignore). (4) Add `Swatinem/rust-cache@v2` to repos missing it. (5) Set `SCCACHE_DIR`, `RUSTC_WRAPPER=sccache`, and mold linker config in workflow `env:` blocks — not in runner config. Track which repos have which optimizations applied using a checklist matrix, and process repos in dependency-layer order (L0 first). Write workflow files via bash heredocs if write_file is jail-restricted.
