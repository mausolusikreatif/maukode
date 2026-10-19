---
name: docker-remote-access-setup
description: "Pattern for exposing Docker localhost services to remote networks via socat forwarding"
run_as_subagent: false
---
When Docker services bind to 127.0.0.1 only, use socat to forward ports to accessible interfaces. First identify all target IPs (Tailscale, LAN, etc.), then run parallel socat instances for TCP and UDP: `socat TCP-LISTEN:PORT,bind=IP,fork TCP:127.0.0.1:PORT &` and `socat UDP-LISTEN:PORT,bind=IP,fork UDP:127.0.0.1:PORT &`. Verify with ss/netstat that listeners exist on all desired interfaces.
