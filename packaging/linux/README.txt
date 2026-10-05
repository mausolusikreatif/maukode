MauKode (Linux build, unsigned, untested on your distro until you try it)

Arch / Omarchy dependencies:
  sudo pacman -S --needed webkit2gtk-4.1 gtk3 libayatana-appindicator openssl git

Install:   ./install.sh        (to ~/.local; ./install.sh --uninstall removes it)
Run:       maukode-desktop     (it starts the background daemon `maukode server start` itself)

Wayland / Hyprland notes:
  - If the window is blank, flickers or crashes (often NVIDIA), start with:
      WEBKIT_DISABLE_DMABUF_RENDERER=1 maukode-desktop
    and, if still broken:  WEBKIT_DISABLE_COMPOSITING_MODE=1 maukode-desktop
  - If it misbehaves under Wayland:  GDK_BACKEND=x11 maukode-desktop
  - Floating/tiling: it is a normal window; add a Hyprland windowrule if you want it floating.

Notes:
  - The daemon outlives the window (agents keep running). Stop it with: maukode server stop
  - Token and URL live in ~/.local/share/maukode/daemon.json (mode 600).
  - Sign in with ChatGPT opens your browser; the approving browser must be on this machine.
  - Please report: does it start, does the sidebar show a green dot, can you create a session.
