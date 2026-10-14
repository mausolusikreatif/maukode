---
name: release-version-bump
description: "Bump version across all project files (Cargo.toml, Cargo.lock, tauri.conf.json, package.json, CHANGELOG) safely without touching third-party dependency versions."
run_as_subagent: false
---
When bumping the project version: (1) Update version in workspace Cargo.toml, app-specific Cargo.toml files, tauri.conf.json, and package.json. (2) For Cargo.lock files, use a script that matches only entries where the `name` field starts with the project's crate prefix (e.g. `maukode-`) before replacing the version on the following line — never use blanket `sed` as it will corrupt third-party deps sharing the same version. (3) Verify no non-project entries were changed by grepping for the old version and confirming remaining hits are all external crates. (4) Update CHANGELOG.md with a new section under `## [X.Y.Z] - YYYY-MM-DD`. (5) Commit all changes together with message `release: vX.Y.Z (#PR)`.
