---
name: pre-bare-metal-disk-audit
description: "Audit disk contents before bare-metal OS installation to identify backup requirements"
run_as_subagent: false
---
Before any bare-metal OS replacement, perform a disk audit: 1) Check home directory sizes with `du`, 2) Identify regenerable artifacts (Rust `target/`, `node_modules/`, `.cache/`, `.cargo/`), 3) List git repos and check which have remotes (safe if pushed), 4) Identify databases and their sizes (docker volumes, pg_dump), 5) Find config files containing secrets (~/.ssh/, ~/.modal.toml, ~/.copilot/, ~/.claude/), 6) Summarize into a table categorizing data as "must backup", "safe if pushed", or "regenerable".
