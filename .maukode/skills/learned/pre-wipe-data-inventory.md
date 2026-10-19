---
name: pre-wipe-data-inventory
description: "Systematically scan disk usage to identify critical vs regenerable data before bare-metal OS reinstallation"
run_as_subagent: false
---
When user plans to wipe disk for OS reinstall, perform data triage: 1) Scan home directory sizes, 2) Identify build artifacts (Rust target/, node_modules/, .cache/) as regenerable - safe to skip, 3) Check git repos for remote status and uncommitted changes, 4) Identify databases in Docker volumes and run proper dump commands, 5) Locate secret/config files (~/.ssh, API keys, credentials), 6) Summarize as table with backup priority: CRITICAL (databases, no-remote repos, secrets), SAFE (pushed repos), SKIP (regenerable artifacts).
