---
name: expose-docker-localhost-to-network
description: "Forward Docker-bound localhost ports to network interfaces using socat without modifying compose files"
run_as_subagent: false
---
To expose a Docker service bound to 127.0.0.1 to network interfaces: 1) Check listening ports with `ss -tlnp` 2) For each target interface IP, run `socat TCP-LISTEN:PORT,bind=INTERFACE_IP,fork TCP:127.0.0.1:PORT &` 3) For UDP, use `socat UDP-LISTEN:PORT,bind=INTERFACE_IP,fork UDP:127.0.0.1:PORT &` 4) Verify with `ss -tlnp | grep PORT` 5) Test actual protocol handshake, not just TCP connect - port accept ≠ service ready
