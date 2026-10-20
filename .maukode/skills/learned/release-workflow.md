---
name: release-workflow
description: "Standard release workflow for maukode: branch, version bump, CHANGELOG, PR, CI, merge, tag, release tarball, install."
run_as_subagent: false
---
To release a new version of maukode: 1) Create branch `release/vX.Y.Z` from an up-to-date `master`. 2) Bump the version as described in `release-version-bump`. 3) Commit as `release: vX.Y.Z`, push, and open a PR. 4) Wait for the `Rust CI` workflow (fmt, tests, TypeScript bindings check, clippy) — never build or test locally (see AGENTS.md); fix failures on the branch. 5) Merge the PR. 6) Pull `master`, create an annotated tag `vX.Y.Z`, and push the tag; this triggers `linux-build.yml`, which builds the tarball with the daemon and the web UI. 7) When the build succeeds, download the tarball from the GitHub release, extract it, and run `./install.sh` (installs into `~/.local`). 8) Verify with `maukode --version` and that the daemon serves the web UI.
