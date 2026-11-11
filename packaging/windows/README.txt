MauKode (Windows x86_64 build, unsigned)

Install:   .\install.ps1       (to %LOCALAPPDATA%\Programs\MauKode; .\install.ps1 -Uninstall removes it)
Run:       maukode server start
Open:      the address it prints (the token is in the URL fragment)

Notes:
  - Install Git for Windows (https://git-scm.com/download/win). The bash tool and hooks run
    commands with its bash.exe, and checkpoints use git. Set MAUKODE_BASH to a bash.exe path to
    use another one. The bash.exe in System32 is the WSL launcher and is not used.
  - The daemon finds the web UI in the web\ folder next to maukode.exe on its own. To serve a
    different build, pass --web-dir <dir> or set MAUKODE_WEB_DIR.
  - Windows SmartScreen may warn about the unsigned maukode.exe; choose "Run anyway".
  - Not available on Windows: the bubblewrap sandbox and lawang cells (both Linux), and
    `maukode update` (re-run the installer instead).
  - Configuration lives in %APPDATA%\maukode; sessions and logs in the same data directory.
  - To reach the daemon from a phone, bind to a Tailscale address instead of 127.0.0.1.
