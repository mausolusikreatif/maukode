---
name: setup-rdp-access-via-socat
description: "Forward RDP or other localhost-bound services to Tailscale/LAN interfaces using socat"
run_as_subagent: false
---
To expose a service bound to localhost (e.g., RDP on 127.0.0.1:3389) to external interfaces, run socat forwarders for both TCP and UDP on each target IP:```bash
# TCP forward to Tailscale IP
socat TCP-LISTEN:<PORT>,bind=<TAILSCALE_IP>,fork TCP:127.0.0.1:<PORT> &
# UDP forward to Tailscale IP
socat UDP-LISTEN:<PORT>,bind=<TAILSCALE_IP>,fork UDP:127.0.0.1:<PORT> &
# Repeat for LAN IP
```
Verify with `ss -tulnp | grep <PORT>` that all forwarders are listening. Keep PID references for cleanup.
