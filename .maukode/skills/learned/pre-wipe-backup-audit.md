---
name: pre-wipe-backup-audit
description: "Audit a machine's disk contents to classify data as regenerable vs critical before OS reinstall"
run_as_subagent: false
---
Scan the machine systematically before wiping. Check: (1) Large regenerable directories — Rust `target/`, JS `node_modules/`, `.cache/`, `.cargo/` — these can be skipped. (2) Databases — dump Postgres/MySQL via `docker exec` or `pg_dump`. (3) Git repos WITHOUT remote configured — these have uncommitted code not backed up anywhere. (4) Secrets — `~/.ssh/`, `~/.config/` with API keys, `.env` files, `~/.modal.toml`, cloud CLI configs. Report a table with category, size, and backup priority (skip/wajib/optional).
