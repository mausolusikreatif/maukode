---
name: remote-access-setup-linux
description: "Systematic inspection of Linux host for remote access options (SSH, RDP, VNC, game streaming)"
run_as_subagent: false
---
When asked about remote access to a Linux machine, systematically check: (1) Tailscale status and connected devices, (2) SSH daemon status and Tailscale SSH capability, (3) Display server type (X11 vs Wayland) and compositor, (4) Installed remote desktop tools (rustdesk, sunshine, xrdp, gnome-remote-desktop), (5) Input device permissions (/dev/uinput, group membership), (6) Existing VMs and their RDP/VNC port bindings. Recommend the simplest option that matches the display server and network topology.
