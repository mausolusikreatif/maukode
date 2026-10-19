---
name: diagnose-tool-execution-failures
description: "Systematically diagnose tool execution failures by categorizing error types (daemon connectivity, configuration, permissions) and testing alternative tools to isolate the root cause."
run_as_subagent: false
---
When tool calls fail, first categorize the error type (daemon unreachable, configuration missing, permission denied). Then test alternative tools that use different execution paths to isolate whether the issue is tool-specific or environment-wide. Document the error progression and resolution steps for future reference.
