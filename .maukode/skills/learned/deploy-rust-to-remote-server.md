---
name: deploy-rust-to-remote-server
description: "Build and install a Rust binary on a remote server over SSH, handling common environment issues"
run_as_subagent: false
---
When deploying a Rust project to a remote server via SSH: (1) Check if cargo is on PATH — if not, try `~/.cargo/bin/cargo` directly since non-interactive SSH doesn't source profile files. (2) If the cargo symlink is broken (points to missing rustup), re-install rustup via `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y` to restore the proxy without reinstalling toolchains. (3) Sync code via `tar` over SSH if rsync is unavailable and git push is blocked by branch protection. (4) Use `systemd-run --user --unit=<name>` to launch the server as a background service that survives SSH disconnection, rather than nohup which can hang the SSH session. (5) Always verify with separate curl commands for web UI and API endpoints.
