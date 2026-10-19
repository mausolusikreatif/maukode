---
name: pre-os-reinstall-inventory
description: "Systematically inventory disk contents to identify backup priorities before wiping a machine for OS reinstall"
run_as_subagent: false
---
Before any bare-metal OS reinstall, perform this inventory sequence: (1) Check disk usage with `du` on home and work directories. (2) Identify regenerable artifacts: Rust `target/` dirs, `node_modules/`, `.cache/`, `.cargo/`. (3) List all git repos and check `git remote -v` to find repos without remotes. (4) Check Docker volumes and dump database contents. (5) Scan for secret files: `.ssh/`, `.modal.toml`, `.copilot/`, `.claude/`, API keys. (6) Present a categorized summary: ❌ safe to skip, ✅ already backed up remotely, ⚠️ requires manual backup.
