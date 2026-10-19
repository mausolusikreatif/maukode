---
name: verify-execution-environment
description: "Check whether bash is running in sandbox or host before performing system administration tasks"
run_as_subagent: false
---
Before executing system modification commands, first verify the execution context by running `hostname && uname -r && cat /etc/os-release` to confirm you are on the expected host machine and not in a sandbox or container environment.
