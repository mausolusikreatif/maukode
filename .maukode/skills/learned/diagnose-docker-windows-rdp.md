---
name: diagnose-docker-windows-rdp
description: "Diagnose RDP connection failures in Docker-based Windows VMs (dockur/windows)"
run_as_subagent: false
---
When RDP connections fail to a Docker-based Windows VM: 1) Test with `xfreerdp3 /v:127.0.0.1:3389 /u:docker /p:admin /cert:ignore /sec:tls` - if this works, the issue is protocol negotiation, not service failure. 2) Check if the VM started with correct storage mounts (bind mounts may be missing if started manually vs via launcher). 3) Test different security protocols: `/sec:tls` (most likely to work), `/sec:rdp` (standard, often rejected), `/sec:nla` (requires Kerberos/AD, fails in Docker). 4) Forward RDP ports via `socat TCP-LISTEN:IP:3389,fork TCP:127.0.0.1:3389` for remote access. 5) For Android clients, ensure they're configured to use TLS security, not auto/standard.
