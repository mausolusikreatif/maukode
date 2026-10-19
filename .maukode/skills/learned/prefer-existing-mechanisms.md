---
name: prefer-existing-mechanisms
description: "Check for existing runtime mechanisms before embedding resources or adding compile-time code"
run_as_subagent: false
---
Before proposing any solution that embeds files into a binary, adds new compile-time constants, or creates new code paths, first grep the codebase for existing runtime discovery mechanisms (e.g., directory scanning, config file loading, plugin systems). If a simpler file-based approach already works, use it instead. State explicitly: "Existing mechanism found at [file:line] — using it instead of embedding." If none exists, proceed with the compile-time approach but justify why runtime discovery is insufficient.
