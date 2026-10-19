---
name: omarchy-windows-vm-direct-start
description: "Start omarchy Windows VM without launcher's pkexec requirement by creating a custom compose file with direct volume mounts"
run_as_subagent: false
---
When needing to start an omarchy Windows VM container without the launcher's pkexec/sudo prompt: 1) Check if `~/.windows/data.img` exists, 2) Create a custom docker-compose.yml that mounts `~/.windows` directly to `/storage` and `~/Windows` to `/shared`, 3) Run `docker compose -f <custom-file> up -d`, 4) Wait for RDP port 3389 to listen, 5) Use socat to forward 3389 to desired network interfaces (Tailscale/LAN). This avoids the launcher's bind mount dependency that requires root privileges.
