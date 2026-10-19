---
name: remote-access-recon
description: "Systematic reconnaissance for setting up remote access between machines via Tailscale/SSH"
run_as_subagent: false
---
When asked to set up remote access between devices, run parallel checks on: (1) `hostname && uname -r && cat /etc/os-release` for machine identity, (2) `ip addr show` for network interfaces, (3) `systemctl is-active sshd && which sshd` for SSH availability, (4) `tailscale status` and `tailscale ssh status` for Tailscale connectivity, (5) `tailscale status` output to list all devices and their IPs. Summarize both endpoints with their IPs, connection status, and what's enabled/disabled before recommending a solution.
