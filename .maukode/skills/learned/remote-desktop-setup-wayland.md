---
name: remote-desktop-setup-wayland
description: "Inspect and set up Sunshine/Moonlight remote desktop streaming on Wayland/Hyprland systems with AMD GPU"
run_as_subagent: false
---
When setting up remote desktop access on a Wayland/Hyprland system, check these items in order: 1) Display server with `echo $XDG_SESSION_TYPE $WAYLAND_DISPLAY`, 2) GPU with `lspci | grep VGA`, 3) VAAPI driver presence with `pacman -Q libva-mesa-driver`, 4) Sunshine availability with `pacman -Si sunshine`, 5) Port conflicts on 47984/47989/47990/47998/48010, 6) `/dev/uinput` permissions (needs `0660` and user in `input` group), 7) Hyprland autostart config location for adding sunshine launch. If sudo is needed and unavailable, output ready-to-run commands grouped by step.
