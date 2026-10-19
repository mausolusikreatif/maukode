---
name: socat-port-forward
description: "Forward localhost-bound service ports to Tailscale/LAN interfaces using socat"
run_as_subagent: false
---
To expose a localhost-bound service to remote networks, use socat port forwarding: `socat TCP-LISTEN:REMOTE_PORT,bind=REMOTE_IP,fork TCP:127.0.0.1:LOCAL_PORT &`. For UDP, use `UDP-LISTEN` instead. Verify with `ss -tlnp | grep PORT`. This avoids modifying Docker compose files and works for both Tailscale (100.x.x.x) and LAN (192.168.x.x) IPs simultaneously.
