MauKode (Linux build, unsigned)

Install:   ./install.sh        (to ~/.local; ./install.sh --uninstall removes it)
Run:       maukode server --bind 127.0.0.1:4096 --token <token> \
             --web-dir ~/.local/share/maukode/web
Open:      http://127.0.0.1:4096#token=<token>

Notes:
  - To reach it from a phone, bind to a Tailscale address instead of 127.0.0.1.
  - The web UI is a static bundle served by the daemon; there is no desktop shell.
