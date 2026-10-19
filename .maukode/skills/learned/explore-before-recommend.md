---
name: explore-before-recommend
description: "When a user asks how to achieve something with their system, first search the codebase for existing capabilities before suggesting new code or external solutions."
run_as_subagent: false
---
When a user asks "how do I do X" or "I need a way to Y" with their codebase, do not immediately propose new features or external tools. Instead: 1) Search the codebase for existing flags, options, configs, or modules related to the requested capability using grep, glob, and read_file. 2) If the capability exists, explain how to use it with concrete examples. 3) Only if it genuinely does not exist, propose what to build. Always cite the specific files and lines where the existing capability is defined.
