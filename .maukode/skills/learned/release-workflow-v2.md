---
name: release-workflow
description: "Standard release workflow for maukode: branch, version bump all files, update CHANGELOG, PR, CI, merge, tag, download, install."
run_as_subagent: false
---
To release a new version of maukode: 1) Create branch `fix/v{VERSION}`. 2) Bump version in `Cargo.toml` (workspace), `apps/desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, `package.json`. 3) Update `Cargo.lock` and Tauri `Cargo.lock` ONLY for `maukode-*` entries using a targeted script (never `sed` globally). 4) Add CHANGELOG entry under `## [v{VERSION}]` with Fixed/Changed sections. 5) Commit, push, open PR. 6) Wait for CI (Rust CI: clippy+test; Linux build on tag). 7) Fix clippy errors if any. 8) Merge PR to master. 9) `git tag v{VERSION}` and push tag to trigger `linux-build.yml`. 10) Wait for release build success.
