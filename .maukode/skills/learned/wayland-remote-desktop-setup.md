---
name: wayland-remote-desktop-setup
description: "Inspects Wayland/Hyprland system capabilities and recommends appropriate remote desktop solution (RustDesk, Sunshine+Moonlight, or Tailscale SSH)"
run_as_subagent: false
---
When setting up remote desktop access on a Wayland/Hyprland system, check: (1) display server via `echo $XDG_SESSION_TYPE` and `echo $WAYLAND_DISPLAY`, (2) GPU info via `lspci | grep -i vga` for encoder support, (3) installed tools via `which rustdesk sunshine moonlight sshd`, (4) Tailscale status via `tailscale status` for network connectivity, (5) input device permissions via `ls -la /dev/uinput` and `groups`, (6) package availability via `pacman -Si rustdesk sunshine`. Recommend RustDesk for general use (simplest setup), Sunshine+Moonlight for low-latency gaming, and Tailscale SSH for terminal-only access. Note that xrdp is NOT recommended for Wayland as it opens separate X11 sessions.
