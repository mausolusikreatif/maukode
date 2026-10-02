# MauKode 🤖

**MauKode** is a high-performance, autonomous agentic AI coding assistant written in **Rust** (inspired by the architecture of [OpenCode](https://opencode.ai) and Claude Code).

MauKode operates autonomously to plan and execute complex software engineering tasks using Large Language Models (LLMs), executing local tools (shell commands, surgical file edits, regex grepping, filesystem navigation), and continuously iterating until the task is solved.

---

## 🌟 Key Highlights

- **Decoupled Client-Server Architecture**:
  - Automatically manages a local background server daemon on `http://127.0.0.1:4096`. Running `maukode` in any terminal immediately connects without opening multiple windows.
  - Supports headless remote server deployment on VPS with optional Bearer token authentication (`maukode --remote http://vps:4096`).
  - Connect multiple terminals across different directories to the same daemon simultaneously.
- **Modern, Sleek Terminal User Interface (TUI)**:
  - Built with `ratatui` and `crossterm`.
  - Non-blocking OS thread polling with zero CPU spikes.
  - Curated semantic theme (*Electric Mint*, *Slate*, *Iris Violet*, *Amber Gold*).
  - High-density 1-line telemetry status bar.
  - Structured tool call cards and syntax-highlighted git diffs (`+` green, `-` red).
  - Floating Telescope-style search modals (`/model`, `/provider`, `/sessions`) with real-time filtering, `ListState` scrolling, and mouse wheel support.
- **Multi-Workspace & Directory Awareness**:
  - Dynamically switch working directories during conversations via `/cd <path>` or inspect with `/dir`.
  - SQLite sessions are indexed and filtered per directory.
- **Universal Multi-Turn Drain Loop**:
  - Automatic tool calling iteration until completion.
  - Built-in tools: `bash` (command runner with timeout), `read_file`, `write_file`, `edit_file` (surgical substring replacement), `grep` (fast regex matching), and `glob`.
- **Multi-Provider & Model Hub**:
  - Native streaming integration for Anthropic, OpenAI, Ollama (Local), OpenRouter, and any OpenAI-compatible proxy (e.g. `llm-kita`).
  - Dynamic model discovery from provider endpoints (`/v1/models`).
- **Persistent SQLite Storage**:
  - Full session history, token analytics, and messages automatically preserved in `~/.local/share/maukode/maukode.db`.

---

## 🏗 Cargo Workspace Architecture

MauKode is structured as a modular Rust monorepo:

```text
maukode/
├── Cargo.toml                    # Workspace root manifest
├── crates/
│   ├── maukode-protocol/         # Core schemas: SessionId, Message, Role, ToolCall, LLMEvent
│   ├── maukode-llm/              # Streaming SSE adapters (Anthropic & OpenAI-compatible)
│   ├── maukode-tools/            # Tool definitions and executors: bash, file, grep, glob
│   ├── maukode-core/             # Session state, ContextManager (AGENTS.md), Drain loop, SQLite Store
│   ├── maukode-server/           # Axum HTTP REST + SSE Server, ServerClient SDK & Daemon manager
│   ├── maukode-tui/              # Interactive Terminal UI (Ratatui, theme, modals, events)
│   └── maukode-cli/              # Main CLI entrypoint (`maukode`)
```

---

## 🚀 Installation

### Prerequisites
- [Rust toolchain](https://rustup.rs/) (Rust 1.80+ or 2021 Edition)

### Install via Cargo

To install the `maukode` binary globally into `~/.cargo/bin`:

```bash
cargo install --path crates/maukode-cli --force
```

Ensure `~/.cargo/bin` is in your `$PATH`.

---

## 💻 Usage

### 1. Launch Interactive TUI (Recommended)
Simply navigate to your project directory and run:

```bash
maukode
```
If the background daemon is not already running, MauKode will automatically start it silently and connect instantly.

### 2. Resume Previous Sessions
```bash
# Continue the latest session in the current directory
maukode -c

# Resume a specific session by UUID
maukode -s <SESSION_UUID>
```

### 3. Server Daemon Management
Control the background server daemon explicitly:

```bash
# Check if the daemon is active, view PID and URL
maukode server status

# Start daemon in the background
maukode server start

# Stop the background daemon
maukode server stop

# Restart the background daemon
maukode server restart

# Run in foreground (ideal for systemd services or Docker containers)
maukode server --bind 0.0.0.0:4096 --token "your-secret-token"
```

### 4. Connect to Remote VPS
```bash
maukode --remote http://vps-ip:4096 --token "your-secret-token"
```

### 5. Headless Single-Prompt Execution
```bash
maukode run "refactor the error handling in crates/maukode-core/src/runner.rs"
```

### 6. Simple CLI REPL (Non-TUI mode)
```bash
maukode --cli
```

### 7. Standalone In-Process Mode
Run without touching any background daemon:
```bash
maukode --standalone
```

---

## ⌨️ Interactive Slash Commands

Inside the TUI, you can type slash commands directly into the prompt:

| Command | Description |
| :--- | :--- |
| `/model` | Open the Telescope model selector modal (dynamic search across provider models). |
| `/model <name>` | Immediately set the active model (e.g. `/model deepseek-v4-pro`). |
| `/provider` | Open the provider menu to choose or add an AI provider. |
| `/provider use <id>` | Switch active provider (e.g. `/provider use ollama`). |
| `/provider add <name> <base_url> [key]` | Register a custom OpenAI-compatible provider. |
| `/cd <path>` | Switch working directory on the fly without closing the session. |
| `/dir` | Print the current working directory. |
| `/sessions` | Open the session history modal to resume previous chats from SQLite. |
| `/new` | Start a fresh session in the current directory. |
| `/clear` | Clear the current conversation viewport. |
| `/help` | Open the quick reference cheat sheet. |
| `/exit`, `/quit` | Exit MauKode. |

---

## 🛡️ Security & Sandboxing

MauKode is built with default-safe execution boundaries to protect your system from unintended damage:

1. **Jail Path Traversal Protection**:
   - All filesystem operations (`read_file`, `write_file`, `edit_file`, `grep`, `glob`, and `cwd`) are strictly sandboxed within the project workspace directory.
   - Any attempt to escape the workspace (e.g., `/etc/`, `~/.ssh/`, or `../../`) is immediately blocked and returned as an access denial to the LLM agent.
2. **Interactive Confirmation Pop-ups (Human-in-the-Loop)**:
   - High-risk or destructive operations (`rm -rf`, `sudo`, `pkill`, `git reset --hard`, `git clean -f`, force push, etc.) automatically pause the turn and trigger an interactive approval modal in the TUI or terminal CLI.
   - Keyboard options:
     - `[y / Enter]`: Allow Once
     - `[a]`: Always Allow for the current session (adds command to session whitelist)
     - `[n / Esc]`: Reject execution (informs agent to find an alternative approach)
3. **Security Policies (`~/.config/maukode/config.json`)**:
   - `permission_mode`: `"ask"` (default), `"strict"` (prompt on all actions), or `"trust"` (auto-allow for CI/CD environments).
   - `jail_path_traversal`: `true` (enforces workspace boundaries).
   - `bash_whitelist`: customizable list of glob patterns for safe commands (e.g. `cargo *`, `npm test`).

---

## 📁 Configuration & Storage

- **Configuration File**: `~/.config/maukode/config.json`  
  Stores active provider, active model, security settings, and configured endpoints.
- **SQLite Database**: `~/.local/share/maukode/maukode.db`  
  Stores all conversation history, metadata, and token usage.
- **Daemon Metadata**: `~/.local/share/maukode/daemon.json`
- **Server Logs**: `~/.local/share/maukode/server.log`

---

## 📜 License

Licensed under either of:
- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
