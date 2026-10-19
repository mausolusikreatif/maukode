---
name: socat-port-forwarder
description: "Forward localhost-bound service ports to external interfaces (Tailscale/LAN) using socat"
run_as_subagent: false
---
To expose a service bound to 127.0.0.1 to external interfaces, use socat: `socat TCP-LISTEN:PORT,bind=EXTERNAL_IP,fork TCP:127.0.0.1:PORT &` for each interface. For UDP, use `UDP-LISTEN` with `UDP`. Verify with `ss -tlnp` or `ss -ulnp`. Run each forwarder in background with `&` and note PIDs for cleanup.
