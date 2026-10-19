---
name: process-resource-audit
description: "Audit a running process's real RAM, CPU, and thread usage using /proc filesystem for accurate reporting beyond ps output."
run_as_subagent: false
---
To audit a process's resource usage: (1) Use `ps aux | grep <process>` to get PID and overview. (2) Read `/proc/<PID>/status` for VmRSS (actual RAM), VmHWM (peak RSS), VmSize/VmPeak (virtual), and Threads. (3) Read `/proc/<PID>/stat` for utime/stime (in jiffies, divide by 100 for seconds). (4) Calculate avg CPU% = (total_cpu_time / uptime_seconds) × 100. (5) For per-thread breakdown, read `/proc/<PID>/task/*/stat`. Distinguish VmRSS (real memory) from VmSize (virtual allocation) when reporting, as virtual size often alarms users unnecessarily.
