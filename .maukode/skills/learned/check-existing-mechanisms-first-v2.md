---
name: check-existing-mechanisms-first
description: "Before implementing a new feature, verify whether an existing mechanism already handles it to avoid over-engineering."
run_as_subagent: false
---
Before writing new code to add content (skills, configs, plugins, templates), first grep the codebase for existing file-based loading or discovery mechanisms. Check: (1) Is there a directory scan that loads `.md`/config files at runtime? (2) Does the existing constructor already accept external files? (3) Would dropping a file into an existing directory achieve the goal without any code changes? Only proceed with code changes (embedding, new APIs, new registration logic) if no existing mechanism can accomplish the task. State which existing mechanism you found and why it does or doesn't suffice before writing any new code.
