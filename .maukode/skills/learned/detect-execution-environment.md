---
name: detect-execution-environment
description: "Quickly determine if bash tool runs in sandbox or host by checking kernel, hostname, and filesystem layout"
run_as_subagent: false
---
When running bash commands, first verify execution context with: `hostname; whoami; uname -r; cat /etc/os-release 2>/dev/null | head -5`. If kernel contains `-gvisor` or hostname differs from expected host, inform user that commands are running in sandbox and may need execution backend switched to Host in settings for host-level operations.
