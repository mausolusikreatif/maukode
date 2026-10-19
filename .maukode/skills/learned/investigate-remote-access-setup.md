---
name: investigate-remote-access-setup
description: "Systematically inspect a Linux host to recommend the best remote access solution (SSH, VNC, Sunshine, RustDesk) based on existing infrastructure."
run_as_subagent: false
---
When asked about remote access to a machine, run these checks in parallel before recommending: 1) Check identity (`hostname`, `uname -r`, OS info), 2) Check network topology (`tailscale status`, `ip addr`, LAN IPs), 3) Check display server (`echo $XDG_SESSION_TYPE`, `echo $WAYLAND_DISPLAY`), 4) Check GPU for encoding capability (`lspci | grep -i vga`, `vainfo`), 5) Check existing tools (`which sshd`, `pacman -Q sunshine rustdesk moonlight`), 6) Check input permissions (`ls -la /dev/uinput`, `groups`). Based on results, rank options by: simplicity of setup, latency requirements, and whether the user needs GUI or terminal-only access. Always check if Tailscale is active first — it often eliminates the need for port forwarding or complex networking.
