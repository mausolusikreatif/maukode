---
name: pre-reinstall-backup-audit
description: "Systematically inspect a machine before OS reinstall to identify what needs backup vs what can be safely discarded."
run_as_subagent: false
---
Before any bare-metal OS reinstall, perform this audit sequence: (1) Scan hardware: CPU virtualization support, RAM, disk model/size, GPU, temperature. (2) Scan disk usage top-level and home directory subdirs. (3) Identify regenerable artifacts: Rust `target/`, `node_modules/`, `.venv/`, `.cache/`, `.cargo/` — mark as safe to skip. (4) List all git repos and check which have remotes (safe if pushed) vs no remote (must backup). (5) Check Docker volumes and containers for databases — run `pg_dump` or equivalent. (6) Scan for secrets: `~/.ssh/`, `~/.modal.toml`, `~/.copilot/`, API keys, `.env` files. (7) Present a summary table with sizes and backup priority: skip / safe-if-pushed / must-backup.
