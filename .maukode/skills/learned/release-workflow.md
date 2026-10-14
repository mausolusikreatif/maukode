---
name: release-workflow
description: "Standard release workflow: branch, version bump all files, update CHANGELOG, PR, CI, merge, tag, download, install."
run_as_subagent: false
---
To release a new version of maukode: 1) Create branch `fix/v{VERSION}`. 2) Bump version in: workspace `Cargo.toml`, `apps/desktop/src-tauri/Cargo.toml`, `tauri.conf.json`, `package.json`. 3) Update Cargo.lock by targeting ONLY `maukode-*` package entries (never bulk sed). 4) Add CHANGELOG entry under `## [v{VERSION}]` with Fixed/Changed sections. 5) Commit all changes including any code fixes. 6) Push and open PR. 7) Wait for CI (clippy + test) to pass — fix any lint errors before merging. 8) Merge PR. 9) Pull master, create annotated tag `v{VERSION}`, push tag. 10) Wait for `linux-build.yml` workflow to complete. 11) Download tarball from GitHub releases, extract, run `./install.sh`. 12) Verify with `maukode --version` and check config values are correct.
