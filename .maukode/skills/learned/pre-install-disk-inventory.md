---
name: pre-install-disk-inventory
description: "Scan disk contents before bare-metal OS install to identify what needs backup vs what's regenerable"
run_as_subagent: false
---
Before any bare-metal OS installation, scan the disk to categorize files: (1) Run `du -sh` on home directory and top-level dirs, (2) Identify regenerable artifacts: Rust `target/`, `node_modules/`, `.cache/`, `.cargo/`, Python `.venv/`, (3) List git repos and check which have remotes vs local-only, (4) Check for Docker volumes with persistent data like databases, (5) List config files with potential secrets (~/.ssh/, ~/.modal.toml, ~/.copilot/). Present a table categorizing: WAJIB backup (data, secrets, repos without remote), SKIP (regenerable), and VERIFY (repos with remote but unpushed branches).
