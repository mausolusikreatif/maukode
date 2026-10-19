---
name: pre-reinstall-disk-triage
description: "Systematically scan disk usage before OS reinstall, categorizing data by backup priority"
run_as_subagent: false
---
Before any bare-metal OS reinstall, run a comprehensive disk inspection: 1) Show top-level home directory sizes, 2) Identify regenerable directories (Rust target/, node_modules/, .venv/, .cache/, .cargo/) and sum their total size, 3) List all git repos and check which have remotes vs which are local-only, 4) Check for Docker volumes and database data, 5) Identify config files with potential secrets (~/.ssh, ~/.modal.toml, ~/.copilot, etc.), 6) Present a categorized inventory table separating "safe to skip" from "must backup" with sizes and priority levels.
