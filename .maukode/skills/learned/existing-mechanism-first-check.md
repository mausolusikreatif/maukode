---
name: existing-mechanism-first-check
description: "Before implementing a feature, evaluate whether an existing runtime mechanism (file discovery, config loading) already handles the requirement without code changes."
run_as_subagent: false
---
Before writing any code to add new content (skills, configs, templates), perform these steps: (1) Check if an existing runtime discovery mechanism already loads files from a user-accessible directory without recompilation. (2) If yes, propose the file-drop approach to the user first: "We can place these as .md files in [directory] — no code changes needed, easy to edit
