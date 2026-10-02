# MauKode 🤖

**MauKode** is a high-performance, autonomous agentic AI coding assistant written in **Rust** (inspired by the architecture of [OpenCode](https://opencode.ai) and Claude Code).

MauKode operates autonomously to plan, research, and execute complex software engineering tasks using Large Language Models (LLMs). It interacts with your codebase via local tools (sandboxed shell commands, surgical file edits, regex grepping, AST code navigation, web search, and MCP integrations) and continuously iterates until the problem is solved.

---

## 🌟 Key Highlights

- **Decoupled Client-Server & Background Daemon**:
  - Automatically manages a local background server daemon on `http://127.0.0.1:4096`. Running `maukode` in any terminal immediately connects with zero startup lag.
  - Supports headless remote server deployment on a VPS with Bearer token authentication (`maukode --remote http://vps:4096 --token <secret>`).
  - Connect multiple terminals across different project directories to the same daemon simultaneously without resource contention.
- **Git Shadow Checkpoints & Rollback Engine (`/undo`, `/diff`)**:
  - Automatically takes shadow git snapshots of your workspace before mutating turns without touching your main `.git` history.
  - Interactive fullscreen visual diff viewer with colored additions (`+`) and deletions (`-`).
  - One-key instant rollback (`[u]`) to revert unintended modifications safely.
- **Tree-Sitter AST Codebase Map (`/map`)**:
  - Multi-language AST parser for **Rust**, **TypeScript/JavaScript**, **Python**, and **Go**.
  - Constructs a PageRank symbol centrality graph to identify core architecture definitions and call hierarchies.
  - Auto-injects an ASCII skeleton map within a strict token budget into the system prompt, giving the LLM immediate architectural awareness.
- **Model Context Protocol (MCP) Client (`/mcp`)**:
  - Full stdio JSON-RPC 2.0 client compliant with Anthropic's Model Context Protocol.
  - Seamlessly bridges external tools from MCP servers (GitHub, SQLite, filesystem, APIs) into the LLM's tool calling schemas.
  - Cascading multi-source configuration (`config.json`, `~/.config/maukode/mcp.json`, project `.maukode/mcp.json`).
- **Smart Context Pruning & Conversation Compaction (`/compact`)**:
  - Real-time token estimator monitoring model context limits (Claude 200k, Gemini 1M, GPT-4o 128k, DeepSeek 64k).
  - Truncates verbose older tool stdout outputs while keeping recent turns intact.
  - Automatic ($>85\%$ usage) or on-demand (`/compact`) summarization preserving key decisions and progress.
  - Full uncompressed conversation history permanently preserved in SQLite `messages_audit` for auditing.
- **Real-Time Web Search & Token-Friendly Documentation Fetcher**:
  - Zero-configuration web search out of the box using DuckDuckGo HTML parser (no API keys required).
  - Pluggable provider support for **Brave Search API**, **Tavily**, and **SearXNG**.
  - Token-efficient web page fetcher converting HTML to clean Markdown, stripping scripts, styles, and boilerplate navigation.
  - Dynamic tool aliasing (`search_web`, `fetch_url`, `read_url`).
- **Standard `AGENTS.md` Memory & Project Detector (`/init`)**:
  - Hierarchical instruction discovery prioritizing standard `AGENTS.md` (with fallbacks to `MAUKODE.md` and global rules).
  - Built-in stack detector automatically recognizes Rust, Node/TS, Python, Go, and CMake projects to generate tailored conventions and commands.
- **Modern, Sleek Terminal User Interface (TUI)**:
  - Built with `ratatui` and `crossterm` with non-blocking OS thread polling (0% idle CPU usage).
  - Semantic theme (*Electric Mint*, *Slate*, *Iris Violet*, *Amber Gold*).
  - High-density telemetry status bar displaying model, provider, tokens, latency, and context ratio.
  - Floating Telescope-style search modals (`/model`, `/provider`, `/sessions`, `/diff`, `/map`, `/mcp`).
- **Security Sandboxing & Human-in-the-Loop Permissions**:
  - Strict workspace directory jail preventing path traversal attacks (`/etc/`, `~/.ssh/`).
  - Interactive approval modals for high-risk commands (`rm -rf`, `sudo`, destructive git operations).
  - Configurable permission policies: `ask` (default), `strict`, and `trust`.
- **Universal Multi-Provider & Model Hub**:
  - Streaming SSE support for Anthropic, OpenAI, Ollama (Local), OpenRouter, DeepSeek, and custom OpenAI-compatible endpoints.

---

## 🏗 Cargo Workspace Architecture

MauKode is structured as a modular Rust monorepo:

```text
maukode/
├── Cargo.toml                    # Workspace root manifest
├── crates/
│   ├── maukode-protocol/         # Core schemas: SessionId, Message, Role, ToolCall, LLMEvent, PermissionDecision
│   ├── maukode-llm/              # Streaming SSE adapters (Anthropic, OpenAI, Ollama, OpenRouter)
│   ├── maukode-tools/            # Built-in tools: bash, file, grep, glob, web_search, fetch_web_content, jail
│   ├── maukode-core/             # Session runner, ContextManager (AGENTS.md, repo map), Checkpoints, MCP, Compaction, SQLite
│   ├── maukode-server/           # Axum HTTP REST + SSE Server, ServerClient SDK & Daemon manager
│   ├── maukode-tui/              # Interactive Terminal UI (Ratatui, modals, diff viewer, map viewer)
│   └── maukode-cli/              # CLI entrypoint (`maukode`), subcommands (`server`, `init`, `run`)
└── docs/                         # Detailed architecture and configuration documentation
```

---

## TUI tests in CI

Run `cargo test --workspace` locally. The TUI tests use Ratatui's in-memory `TestBackend` and deterministic chat chunks; they need no terminal, API key, LLM, or fake HTTP server. They cover word wrapping in chat, the multi-row input and dialogs, top and bottom of a 2,000-message scrollback, and burst streaming (4,000 chunks with 40 redraws). An end-to-end TUI test sends keyboard and LLM events through the app, then inspects a full virtual terminal at narrow and wide sizes, including permission-command scrolling and provider URL entry. Generous time limits in the stream tests catch severe regressions on CI; use a dedicated benchmark runner for precise latency comparisons.

The viewport uses Ratatui's own wrapped-line count to calculate scroll limits. The current `Paragraph` scroll API accepts a `u16` row offset, so histories beyond 65,535 visual rows need a separate windowing implementation to reach the newest rows reliably.

## 🚀 Installation

### Prerequisites
- [Rust toolchain](https://rustup.rs/) (Rust 1.80+ or 2021 Edition)

### Install via Cargo
To compile and install the `maukode` binary globally into `~/.cargo/bin`:

```bash
cargo install --path crates/maukode-cli --force
```

Ensure `~/.cargo/bin` is in your `$PATH`.

### Use a ChatGPT plan

MauKode offers **ChatGPT plan** and **OpenAI API key** as separate providers. To
use an eligible ChatGPT Plus or Pro plan, run:

```bash
maukode auth login
maukode auth status
maukode
```

The login command opens the official Sign in with ChatGPT page in your browser.
Approve use of your ChatGPT plan there. MauKode selects an available model and
activates the `chatgpt` provider. In the TUI, `/provider` lets you switch between
ChatGPT plan and an API-key provider; `maukode -p chatgpt` selects the plan from
the CLI. Run `maukode auth logout` to revoke the active session, or
`maukode auth login --new-account` to connect another account. List saved
accounts with `maukode auth accounts`, then switch with
`maukode auth use <CLIENT_ID>`.

Credentials are stored separately from API keys in
`~/.config/maukode/chatgpt_auth.json` (or the platform's configuration
directory). On Unix, the file is owner-readable only. API-key usage has its
own billing and still uses the `openai` provider. ChatGPT plan usage currently
works with the local daemon or standalone mode; remote-server credential
transfer is not yet implemented.

The implementation follows [OpenAI's open-source Sign in with ChatGPT
guide](https://developers.openai.com/siwc/token-sharing-open-source) and uses
the public Responses API. Login requires a browser on the same machine as the
MauKode CLI because the callback listens on `127.0.0.1`.

---

## 💻 Usage & CLI Modes

### 1. Interactive TUI (Default Mode)
Navigate to any project directory and start MauKode:

```bash
maukode
```
If the background daemon is not yet running, MauKode starts it silently in the background and connects immediately.

### 2. Resume Previous Sessions
```bash
# Continue the latest session in the current directory
maukode -c

# Resume a specific session by UUID
maukode -s <SESSION_UUID>
```

### 3. Initialize Project Memory (`AGENTS.md`)
Detect the workspace tech stack and generate an idiomatic `AGENTS.md` rules file:

```bash
maukode init

# Force overwrite existing AGENTS.md
maukode init --force
```

### 4. Headless Single-Prompt Execution
Execute a task non-interactively from the shell and exit upon completion:

```bash
maukode run "refactor the error handling in crates/maukode-core/src/runner.rs"
```

### 5. Server Daemon Management
Control the background server daemon explicitly:

```bash
# Check status, PID, and listening URL
maukode server status

# Start daemon in the background
maukode server start

# Stop the background daemon
maukode server stop

# Restart the background daemon
maukode server restart

# Run daemon in foreground (ideal for systemd or Docker)
maukode server --bind 0.0.0.0:4096 --token "your-secret-token"
```

### 6. Connect to Remote VPS Daemon
```bash
maukode --remote http://vps-ip:4096 --token "your-secret-token"
```

### 7. Non-TUI Terminal REPL & Standalone Mode
```bash
# Run in plain readline CLI mode (ideal for simple terminals)
maukode --cli

# Run in standalone mode without spawning or touching any background daemon
maukode --standalone
```

---

## ⌨️ Interactive Slash Commands

Inside the TUI or CLI REPL, type slash commands directly into the prompt:

| Command | Description |
| :--- | :--- |
| `/model` | Open Telescope model selector modal (dynamic search across provider models). |
| `/model <name>` | Immediately switch active model (e.g. `/model claude-3-7-sonnet-20250219`). |
| `/provider` | Open provider modal to select, view, or add AI providers. |
| `/provider use <id>` | Switch active provider (e.g. `/provider use ollama`). |
| `/provider add <name> <base_url> [key]` | Register a custom OpenAI-compatible provider. |
| `/diff` | Open interactive visual diff viewer comparing workspace against shadow checkpoints. |
| `/undo` | Roll back workspace file modifications made by the AI to the last checkpoint. |
| `/map` | Open interactive fullscreen Tree-Sitter codebase symbol hierarchy viewer. |
| `/mcp` | List connected Model Context Protocol servers and their available tools. |
| `/compact` | Trigger manual conversation compaction to summarize history and free token context. |
| `/init` | Detect project stack and generate or update the workspace `AGENTS.md`. |
| `/sessions` | Open session history modal to browse and switch between previous chats. |
| `/cd <path>` | Change working directory on the fly without closing the current session. |
| `/dir` | Print the current working directory. |
| `/new` | Start a fresh session in the current directory. |
| `/clear` | Clear the current conversation viewport. |
| `/help` | Open the quick reference cheat sheet modal. |
| `/exit`, `/quit` | Exit MauKode. |

---

## 🛠️ Autonomous Tool Suite

MauKode provides the AI agent with a comprehensive set of secure local and online tools:

| Tool Name | Aliases | Description |
| :--- | :--- | :--- |
| `read_file` | — | Read the contents of a file within the workspace. |
| `write_file` | — | Create a new file or overwrite an existing file. |
| `edit_file` | — | Perform surgical exact-match substring replacements on existing files. |
| `bash` | — | Execute shell commands within the sandboxed workspace directory with timeout limits. |
| `grep` | — | Search file contents using fast regular expressions. |
| `glob` | — | Find files matching wildcard patterns (e.g. `src/**/*.rs`). |
| `web_search` | `search_web` | Search the web for up-to-date documentation and library references (DuckDuckGo, Brave, Tavily). |
| `fetch_web_content` | `fetch_url`, `read_url` | Download a web page and convert HTML into clean, token-efficient Markdown. |
| `get_codebase_map` | — | Query the Tree-Sitter AST symbol hierarchy for a specific directory. |
| *External MCP Tools* | — | Any tools exposed by connected MCP servers (e.g. GitHub issues, SQLite queries). |

---

## 🛡️ Security & Sandboxing

MauKode enforces strict safety boundaries to prevent unintended system modifications:

1. **Jail Path Traversal Protection**:
   - All filesystem operations are strictly sandboxed within the project workspace.
   - Any attempt to escape the workspace (e.g., `/etc/`, `~/.ssh/`, or `../../`) is immediately blocked and returned as an access denial.
2. **Interactive Human-in-the-Loop Confirmation**:
   - Destructive operations (`rm -rf`, `sudo`, `pkill`, `git reset --hard`, `git clean -f`, force push) trigger an approval modal:
     - `[y / Enter]`: Allow Once
     - `[a]`: Always Allow for the current session (adds command to session whitelist)
     - `[n / Esc]`: Reject execution (informs agent to choose another approach)
3. **Configurable Permission Policies**:
   - `"ask"` (default): Prompts for dangerous commands.
   - `"strict"`: Prompts for all commands and file operations.
   - `"trust"`: Auto-allows all commands (intended for isolated Docker / CI containers).

---

## 📚 Further Documentation

For in-depth guides and technical details:
- **[Configuration Guide (docs/CONFIGURATION.md)](docs/CONFIGURATION.md)**: Complete reference for `config.json`, MCP server setups, web search API keys, and compaction thresholds.
- **[Architecture Deep Dive (docs/ARCHITECTURE.md)](docs/ARCHITECTURE.md)**: Detailed breakdown of the daemon HTTP/SSE protocol, shadow checkpoint engine, Tree-Sitter PageRank algorithm, and SQLite auditing.

---

## 📜 License

Licensed under either of:
- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
