---
name: existing-mechanism-first-check
description: "Before implementing a new feature, verify whether existing infrastructure already supports it without code changes."
run_as_subagent: false
---
Before writing any new code to implement a feature request, perform these checks: (1) Search the codebase for existing directories, loaders, or config paths that could serve the same purpose (e.g. `load_skills_from_dir`, `user_skills_dirs`, `workspace_skills_dirs`). (2) Determine if the feature can be achieved by simply placing files in an existing discovery path. (3) If a no-code solution exists, prefer it and explain why — it avoids recompilation, is easier to modify, and uses tested code paths. Only proceed with code changes if the existing mechanism genuinely cannot support the requirement.
