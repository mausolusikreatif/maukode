---
name: inspect-process-resources
description: "Inspect detailed memory and CPU usage of a running Linux process via /proc filesystem."
run_as_subagent: false
---
To analyze a process by PID, read /proc/<pid>/status for memory metrics (VmRSS=actual RAM, VmHWM=peak RSS, VmSize=virtual, VmPeak=peak virtual) and /proc/<pid>/stat for CPU times (fields 14=utime, 15=stime in clock ticks; divide by sysconf(_SC_CLK_TCK) for seconds). Calculate average CPU% as (utime_sec+stime_sec)/uptime_sec×100. Use /proc/<pid>/task/*/stat to inspect per-thread CPU. Always distinguish RSS (physical RAM) from VmSize (virtual address space) when reporting.
