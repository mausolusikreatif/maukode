---
name: release-version-bump
description: "Bump version across all project files (Cargo.toml, Cargo.lock, tauri.conf.json, package.json, CHANGELOG) safely without touching third-party dependency versions."
run_as_subagent: false
---
When bumping the project version: 1) Update `version = "X.Y.Z"` in workspace `Cargo.toml` and Tauri `Cargo.toml`. 2) Update `"version"` in `tauri.conf.json` and `package.json`. 3) In `Cargo.lock` files, use a Python script that matches lines `name = "maukode-*"` and updates only the following `version =` line — never use blanket `sed` as it corrupts third-party deps sharing the same version. 4) Add a CHANGELOG entry under `## [X.Y.Z]` with Fixed/Changed sections. 5) Verify no non-maukode entries were changed by grepping for the old version in Cargo.lock.
