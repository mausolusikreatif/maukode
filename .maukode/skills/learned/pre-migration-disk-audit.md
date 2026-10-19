---
name: pre-migration-disk-audit
description: "Systematically audit disk contents before bare-metal OS reinstall to ensure no important data is lost"
run_as_subagent: false
---
Before any disk-wiping operation (OS reinstall, bare-metal hypervisor install), perform a systematic audit: 1) Scan top-level directory sizes with `du`, 2) Identify regenerable artifacts (Rust `target/`, `node_modules/`, `.cache/`, `.cargo/`, Python `.venv/`) and exclude from backup estimate, 3) Check each git repo for remote status and uncommitted/unpushed changes, 4) Identify databases (Docker volumes, local DBs) and their sizes, 5) Locate secret/config files (`~/.ssh/`, `~/.modal.toml`, `~/.copilot/`, API tokens), 6) Produce a categorized summary table with backup priority levels.
