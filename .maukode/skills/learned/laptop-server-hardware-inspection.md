---
name: laptop-server-hardware-inspection
description: "Comprehensive hardware inspection for converting a laptop into a headless server"
run_as_subagent: false
---
Run the following checks when evaluating a laptop for server conversion: CPU virtualization support (grep -E "svm|vmx" /proc/cpuinfo, ls /dev/kvm), memory availability (free -h), storage layout (lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINT,MODEL), network interfaces with status (ip -br link), GPU and thermal sensors (lspci -k, sensors), battery health (upower -i /dev/battery, cat /sys/class/power_supply/BAT*/status), BIOS/UEFI version and Secure Boot status, and kernel boot parameters (cat /proc/cmdline). Summarize findings in a table format with server-specific recommendations.
