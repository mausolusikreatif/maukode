---
name: docker-vm-control-plane
description: "Create a CLI + web panel for managing Docker-based VMs with automatic port forwarding to Tailscale/LAN"
run_as_subagent: false
---
When setting up Docker VMs that need remote access: 1) Create a CLI script at `~/.local/bin/<name>` with start/stop/status/forward/unforward commands 2) Create a Python HTTP panel at `~/.config/<name>/panel.py` on port 8008 with button endpoints 3) Create a port forwarding wrapper script using socat to forward RDP (3389) and web UI (8006) ports to both Tailscale IP and LAN IP 4) Set up systemd user services for auto-start: one for the panel (Type=simple), one for forwarders (use a wrapper script since socat backgrounds itself) 5) Test all endpoints via curl before declaring success 6) Verify RDP works end-to-end using FreeRDP with `/sec:tls /cert:ignore` flags, not raw handshake tests
