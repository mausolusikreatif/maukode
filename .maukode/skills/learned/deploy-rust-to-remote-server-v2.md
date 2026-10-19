---
name: deploy-rust-to-remote-server
description: "Build and install a Rust binary on a remote server via SSH, handling common environment issues"
run_as_subagent: false
---
When deploying a Rust project to a remote server via SSH: (1) Check if cargo is available with `which cargo` or `~/.cargo/bin/cargo --version` — if the rustup proxy symlink is broken, restore it by piping the rustup installer script. (2) Transfer source code via `tar c -C <dir> . | ssh <host> 'tar x -C <dest>'` since rsync may not be installed. (3) Build with `cargo install --path <crate> --locked` from the workspace root. (4) Run long-running servers with `systemd-run --user --unit=<name> -- <binary> <args>` to avoid SSH session hangs. (5) Verify with separate SSH commands using curl, not by running the server in the foreground.
