---
name: split-large-audit-subagents
description: "Split large codebase audit tasks across multiple focused subagents to avoid context exhaustion"
run_as_subagent: false
---
When auditing a large codebase across many files and dimensions, do not assign all work to a single subagent. Instead, split into 2-3 focused subagents each covering a specific scope (e.g., one for design system/CSS tokens, one for component-level issues, one for accessibility). Each subagent should read only the files in its scope and report findings with file:line references and severity grouping. Run subagents in parallel when independent.
