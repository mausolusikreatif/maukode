---
name: release-version-bump
description: "Bump the project version (Cargo.toml, Cargo.lock, apps/desktop package.json and package-lock.json, CHANGELOG) without touching third-party dependency versions."
run_as_subagent: false
---
When bumping the project version: 1) Update `version = "X.Y.Z"` under `[workspace.package]` in the root `Cargo.toml` (all crates use `version.workspace = true`). 2) Update `"version"` in `apps/desktop/package.json` and the two top-level `"version"` entries of `apps/desktop/package-lock.json` (the root object and `packages[""]`). 3) In `Cargo.lock`, only change the `version =` line that follows a `name = "maukode-*"` line, with a script (see `safe-cargo-lock-version-bump`) — never a blanket `sed`, which corrupts third-party crates that share the version. 4) Move the `## [Unreleased]` entries of `CHANGELOG.md` into a new `## [X.Y.Z] - YYYY-MM-DD` section. 5) Verify with `git diff` that no non-maukode entries changed. There is no Tauri shell any more: there is no `tauri.conf.json` or `src-tauri` to update.
