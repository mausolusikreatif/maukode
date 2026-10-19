---
name: linux-process-resource-analysis
description: "Analyze a running Linux process's memory, CPU, and thread usage using /proc filesystem for detailed diagnostics."
run_as_subagent: false
---
To inspect a process by PID: (1) Run `ps -p <PID> -o pid,user,%cpu,%mem,rss,etime,comm` for a quick overview. (2) Read `/proc/<PID>/status` for VmRSS, VmHWM, VmSize, VmPeak, RssAnon, RssFile, and Threads. (3) Read `/proc/<PID>/stat` fields: utime (field 14), stime (field 15), starttime (field 22) in clock ticks (usually 100 Hz) to compute total CPU time and average CPU%. (4) For per-thread breakdown, iterate `/proc/<PID>/task/*/stat` and sort by utime+stime. Always present VmRSS as the actual RAM figure and compute avg CPU% = (utime+stime)/uptime × 100.
