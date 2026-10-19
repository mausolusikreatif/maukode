---
name: check-existing-mechanisms-first
description: "Before implementing a new feature, verify whether an existing mechanism already solves the problem with less code."
run_as_subagent: false
---
Before writing any code to add or ship content (skills, configs, templates), first search the codebase for existing discovery/loading mechanisms (e.g., directory-based loading, file watchers, plugin systems). Ask: "Can this be achieved by simply placing a file in an existing directory?" Only proceed with code changes if no existing mechanism suffices. State which existing mechanism you considered and why it falls short before writing new code.```
