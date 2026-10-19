---
name: environment-detection-before-advice
description: "Verify execution environment (sandbox vs host) before providing system administration guidance"
run_as_subagent: false
---
Before giving system configuration advice, run hostname, uname -a, and check /proc/1/cgroup to confirm whether executing in a sandbox/container or directly on the host. If sandboxed, inform the user that commands may not reflect their actual system and ask them to run commands directly if needed.
