---
name: analyze-process-resources
description: "Given a PID or process name, produce a detailed breakdown of memory and CPU usage using the /proc filesystem."
run_as_subagent: false
---
To analyze a process's resource usage in detail: 1) Find the PID via `ps aux | grep <process>`. 2) Read `/proc/<PID>/status` for VmRSS, VmHWM, VmSize, VmPeak, RssAnon, RssFile, and Threads. 3) Read `/proc/<PID>/stat` and extract utime (field 14) and stime (field 15) in jiffies (divide by 100 for seconds). 4) Calculate uptime from `/proc/<PID>/stat` starttime (field 22) and current uptime from `/proc/uptime`. 5) Compute average CPU% = (utime+stime) / uptime × 100. 6) Per-thread breakdown: list `/proc/<PID>/task/*/stat` for top consumers. Present results in a table with clear explanations of each metric.
