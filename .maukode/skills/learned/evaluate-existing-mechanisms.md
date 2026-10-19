---
name: evaluate-existing-mechanisms
description: "Before implementing a new feature, audit existing mechanisms that could accomplish the goal with minimal or zero code changes."
run_as_subagent: false
---
When asked to add a new capability, first enumerate all existing runtime mechanisms (config dirs, file-based discovery, plugin systems, user overrides) that could achieve the goal without code changes. Compare the complexity of each approach. Prefer the approach with the fewest code changes and the most flexibility for the end user. Only propose new infrastructure if no existing mechanism is sufficient. Present the comparison to the user before implementing.
