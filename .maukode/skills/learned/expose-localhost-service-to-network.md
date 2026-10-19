---
name: expose-localhost-service-to-network
description: "Forward a localhost-bound service port to Tailscale or LAN interfaces using socat"
run_as_subagent: false
---
When a service is bound to 127.0.0.1 and needs to be accessible remotely, use socat to create TCP and UDP listeners on target interfaces that forward to localhost. First identify the target IPs (e.g., `tailscale ip -4` for Tailscale, `ip addr show` for LAN). Then run: `socat TCP-LISTEN:PORT,bind=TARGET_IP,fork TCP:127.0.0.1:LOCAL_PORT &` for TCP and `socat UDP-LISTEN:PORT,bind=TARGET_IP,fork UDP:127.0.0.1:LOCAL_PORT &` for UDP. Verify with `ss -tlnp` and `ss -ulnp`. This avoids modifying service configuration files.
