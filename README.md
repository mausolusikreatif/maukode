# MauKode 🤖

**MauKode** is an autonomous AI coding agent written in **Rust**, inspired by the architecture of
[OpenCode](https://opencode.ai) and Claude Code.

It plans, researches, and executes software engineering tasks with a Large Language Model. The agent
works through local tools (shell commands, file edits, regex search, a Tree-Sitter code map, web
search, MCP servers, and subagents) and keeps iterating until the task is done. A local daemon runs
the agent; a terminal UI, a plain CLI REPL, and a browser-based web UI are clients of that daemon.

> Status of every feature, and the known gaps, live in **[docs/ROADMAP.md](docs/ROADMAP.md)**.

---

## 🌟 Key Highlights

- **Client–server daemon**
  - `maukode` starts (or reuses) a background daemon on `http://127.0.0.1:4096` and connects to it.
  - Several terminals, project directories, and the web UI can share one daemon.
  - The daemon can run on a remote host with Bearer-token authentication
    (`maukode --remote http://vps:4096 --token <secret>`).
- **Web UI** (`apps/desktop`)
  - A React single-page app served by the daemon. It is a browser app; there is no native desktop
    shell.
  - Overview of all agents by urgency, a cross-session approval inbox, session threads with plans
    and subagents, a change review panel, and full Settings. It also works on phones.
- **Shadow checkpoints and rollback** (`/undo`, `/diff`)
  - Before each turn, the workspace is snapshotted into a separate shadow git repository, so your
    own `.git` history is never touched.
  - A diff viewer and a one-key rollback revert unwanted agent changes.
  - Optional **git worktree isolation** per session, with commit and merge from the web UI.
- **Tree-Sitter codebase map** (`/map`)
  - Symbol extraction for **Rust**, **TypeScript/JavaScript**, **Python**, and **Go**.
  - A PageRank symbol graph ranks central definitions; a skeleton within a token budget (800 tokens)
    is added to the system prompt.
- **Project memory and knowledge store**
  - Hierarchical instructions from `AGENTS.md` (with `MAUKODE.md`/`RULES.md` and global fallbacks);
    `maukode init` generates one for Rust, Node/TS, Python, Go, and CMake projects.
  - A per-project knowledge store in SQLite with `memory_*` tools for the agent (search, write,
    history, links, graph). Optional post-turn reflection and memory consolidation.
    See [docs/MEMORY_ARCHITECTURE.md](docs/MEMORY_ARCHITECTURE.md).
- **Subagents**
  - `invoke_subagent` runs a child agent with its own budget; its progress streams into the parent
    session as cards with a drill-down trace. See
    [docs/SUBAGENT_ARCHITECTURE.md](docs/SUBAGENT_ARCHITECTURE.md).
- **Model Context Protocol (MCP) client** (`/mcp`)
  - stdio JSON-RPC 2.0 client; tools from MCP servers are added to the agent's tool list.
  - Configuration from `config.json`, `~/.config/maukode/mcp.json`, and project `.maukode/mcp.json`.
- **Context pruning and compaction** (`/compact`)
  - Older tool outputs are truncated in the request payload while recent turns stay intact.
  - Automatic summarization above 85% of the model's context window, or on demand with `/compact`.
  - The uncompacted history is kept in the SQLite `messages_audit` table.
- **Web search and page fetcher**
  - DuckDuckGo works without an API key; Brave, Tavily, and SearXNG are also supported.
  - Fetched HTML is converted to compact Markdown.
- **Terminal UI**
  - Built with `ratatui` and `crossterm`; the screen is only redrawn when something changes.
  - Status bar with model, provider, tokens, and context usage; Telescope-style modals for models,
    providers, sessions, diffs, the code map, MCP, plans, and subagents.
- **Permissions and execution isolation**
  - Approval prompts for risky actions, configurable with four permission modes.
  - File tools are confined to the workspace by a path jail.
  - Shell commands run on the host by default. They can instead run under bubblewrap, or in an
    isolated **lawang** cell (bubblewrap, gVisor, or a Firecracker VM).
    See *Security and Execution* below for what each layer does and does not protect.
- **Providers**
  - Streaming support for Anthropic and OpenAI-compatible APIs (OpenAI, OpenRouter, Ollama,
    DeepSeek, llm-kita, or any custom endpoint), plus a **ChatGPT plan** provider via Sign in with
    ChatGPT.

---

## 🏗 Workspace Layout

```text
maukode/
├── Cargo.toml                 # Workspace manifest
├── crates/
│   ├── maukode-protocol/      # Shared types: SessionId, Message, ContentPart, LLMEvent, permissions
│   ├── maukode-llm/           # Streaming adapters: Anthropic and OpenAI-compatible
│   ├── maukode-tools/         # Built-in tools, path jail, bash risk assessment, exec backends
│   ├── maukode-memory/        # Project knowledge store and memory_* tools
│   ├── maukode-core/          # Session runner, context, repo map, checkpoints, MCP, compaction,
│   │                          #   subagents, skills/plugins, SQLite session store, ChatGPT auth
│   ├── maukode-server/        # Axum HTTP + SSE daemon, client SDK, daemon manager
│   ├── maukode-tui/           # Terminal UI (ratatui)
│   └── maukode-cli/           # `maukode` binary and subcommands
├── apps/desktop/              # Web UI (React + TypeScript), served by the daemon
├── packaging/linux/           # Release tarball installer
└── docs/                      # Architecture, configuration, API, and roadmap documents
```

---

## 🚀 Installation

### From a release (Linux x86_64)

```bash
curl -fsSL https://raw.githubusercontent.com/mausolusikreatif/maukode/master/install.sh | bash
```

This downloads the latest release, verifies its sha256, and installs the daemon and the web UI into
`~/.local` (`--version vX.Y.Z` picks a release, `bash -s -- --uninstall` removes it). Then run
`maukode server --token <token>`; the web UI is served from the same address. To install by hand,
download the tarball from the GitHub release, extract it, and run `./install.sh`. See
`packaging/linux/README.txt`.

### From source

Requires the [Rust toolchain](https://rustup.rs/) (stable, 2021 edition):

```bash
cargo install --path crates/maukode-cli --force
```

Make sure `~/.cargo/bin` is in your `$PATH`. To serve the web UI from a source checkout, build it
with `npm ci && npm run build` in `apps/desktop`; see [apps/desktop/README.md](apps/desktop/README.md).

### Providers and API keys

Built-in providers read their keys from `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
`OPENROUTER_API_KEY`, and `LLMKITA_API_KEY`, or from `~/.config/maukode/config.json`. Use
`/provider` in the TUI or Settings in the web UI to switch providers and add custom
OpenAI-compatible endpoints. See [docs/CONFIGURATION.md](docs/CONFIGURATION.md).

### Use a ChatGPT plan

MauKode offers **ChatGPT plan** and **OpenAI API key** as separate providers. To use an eligible
ChatGPT Plus or Pro plan, run:

```bash
maukode auth login
maukode auth status
maukode
```

The login command opens the official Sign in with ChatGPT page in your browser. Approve use of
your ChatGPT plan there. MauKode selects an available model and activates the `chatgpt` provider.
In the TUI, `/provider` switches between the ChatGPT plan and an API-key provider; `maukode -p
chatgpt` selects the plan from the CLI. Run `maukode auth logout` to revoke the active session, or
`maukode auth login --new-account` to connect another account. List saved accounts with
`maukode auth accounts`, then switch with `maukode auth use <CLIENT_ID>`.

Credentials are stored separately from API keys in `~/.config/maukode/chatgpt_auth.json` (or the
platform's configuration directory). On Unix, the file is owner-readable only. API-key usage has
its own billing and still uses the `openai` provider. ChatGPT plan usage works with the local
daemon or standalone mode; transferring credentials to a remote server is not implemented yet.

The implementation follows [OpenAI's open-source Sign in with ChatGPT
guide](https://developers.openai.com/siwc/token-sharing-open-source) and uses the public Responses
API. Login requires a browser on the same machine as the MauKode CLI because the callback listens
on `127.0.0.1`.

---

## 💻 Usage

### Interactive TUI (default)

```bash
cd your-project
maukode
```

If the daemon is not running yet, MauKode starts it in the background and connects to it.

### Resume sessions

```bash
maukode -c                  # continue the latest session in this directory
maukode -s <SESSION_UUID>   # resume a specific session
```

### Generate `AGENTS.md`

```bash
maukode init           # detect the stack and write AGENTS.md
maukode init --force   # overwrite an existing AGENTS.md
```

### Single prompt, non-interactive

```bash
maukode run "refactor the error handling in crates/maukode-core/src/runner.rs"

# For CI and scripts: one JSON result object (stream-json: every event as a JSON line first)
maukode run --output json "update the changelog for the last release" | jq .result
maukode run --output stream-json --permission-mode trust "run the test suite and fix failures"
```

With `--output json` or `stream-json`, only JSON is printed. The result object holds
`session_id`, `stop_reason`, `is_error`, `result` (the assistant's text), `tool_calls`,
`denied_permissions`, `errors`, and `usage` (tokens and `cost_microusd`). Calls that need approval
are denied, because nobody can answer the prompt; `--permission-mode` (for example `trust`)
changes that for this run. The exit code is 1 when the turn failed.

### Daemon management

```bash
maukode server status    # PID and URL
maukode server start     # start in the background (--port, --token)
maukode server stop
maukode server restart

# Run in the foreground (systemd, Docker). Always set a token on a non-loopback address.
maukode server --bind 0.0.0.0:4096 --token "your-secret-token" --web-dir /path/to/web
```

### Web UI

When the daemon finds a web bundle (`--web-dir`, `MAUKODE_WEB_DIR`, or a `web/` directory next to
the binary), it serves the UI on the same address. Open the address that `maukode server status`
prints (`http://127.0.0.1:4096#token=<token>`).

### Trust a project's MCP servers

```bash
maukode trust            # allow this folder's mcp.json / .maukode/mcp.json to start MCP servers
maukode trust --revoke   # undo
```

### Remote daemon

```bash
maukode --remote http://vps-ip:4096 --token "your-secret-token"
```

### Plain REPL and standalone mode

```bash
maukode --cli          # line-based REPL instead of the TUI
maukode --standalone   # run in-process, without starting or using the daemon
```

Other flags: `-p/--provider`, `-m/--model`, `--api-key`, `--base-url`, `-C/--dir`, and
`--max-steps`. Run `maukode --help` for the full list.

---

## ⌨️ Slash Commands

Available in the TUI and the CLI REPL:

| Command | Description |
| :--- | :--- |
| `/model`, `/models` | Open the model picker. |
| `/model <name>` | Switch to a model directly. |
| `/provider`, `/providers` | Open the provider picker (select, view, or add a provider). |
| `/provider use <id>` | Switch the active provider (e.g. `/provider use ollama`). |
| `/provider add <name> <base_url> [key]` | Register a custom OpenAI-compatible provider. |
| `/permission [ask\|strict\|review\|trust]` | Show or change the permission mode (alias `/privacy`). |
| `/diff` | Diff the workspace against the session's checkpoint. |
| `/undo` | Restore files to the previous checkpoint. |
| `/map`, `/repomap` | Open the Tree-Sitter code map viewer. |
| `/mcp` | List connected MCP servers and their tools. |
| `/compact` | Summarize older history to free context. |
| `/steer <text>` | Add guidance to the running turn (also `Alt+Enter`). |
| `/queue [run\|clear]` | List prompts queued during a turn (`Enter` while it runs), send the next, or drop them. |
| `/plan` | Toggle the plan widget (also `F2`). |
| `/subagents`, `/subagent` | Open the subagent trace viewer. |
| `/audit` | Show the session's permission and compaction audit log. |
| `/init` | Generate or update `AGENTS.md`. |
| `/sessions` | Browse and switch sessions. |
| `/new` | Start a new session in the current directory. |
| `/cd <path>` | Change the working directory. |
| `/dir`, `/pwd` | Print the working directory. |
| `/clear` | Clear the chat view. |
| `/help` | Open the quick reference. |
| `/exit`, `/quit` | Exit MauKode. |

---

## 🛠️ Agent Tools

| Group | Tools | Notes |
| :--- | :--- | :--- |
| Files | `read_file`, `write_file`, `edit_file`, `glob`, `grep` | Confined to the workspace by the path jail. `edit_file` does exact-match replacements. |
| Shell | `bash` | Runs on the host, under bubblewrap, or in a lawang cell (see below). Has a timeout. |
| Web | `web_search` (alias `search_web`), `fetch_web_content` (aliases `fetch_url`, `read_url`) | DuckDuckGo by default; Brave, Tavily, SearXNG. |
| Code map | `get_codebase_map` | Tree-Sitter symbol skeleton for a directory. |
| Memory | `memory_search`, `memory_get`, `memory_write`, `memory_update`, `memory_delete`, `memory_restore`, `memory_history`, `memory_browse`, `memory_catalog`, `memory_graph`, `memory_suggest_links`, `memory_edge_health` | Project knowledge store. |
| Subagents | `invoke_subagent` | Delegates a task to a child agent. |
| MCP | *any* | Tools exposed by configured MCP servers. |

---

## 🛡️ Security and Execution

MauKode has several protective layers. Each one covers a specific risk, and none of them is a
complete sandbox on its own:

1. **Path jail (file tools).** `read_file`, `write_file`, `edit_file`, `glob`, and `grep` resolve
   paths (including symlinks) and refuse anything outside the workspace or
   `security.allowed_external_paths`. The jail does **not** apply to `bash`.
2. **Approval prompts.** Depending on the permission mode, a tool call waits for your decision:
   - `[y / Enter]` allow once
   - `[a]` allow for the rest of the session (exact command)
   - `[p]` run privately: you see the result, the model gets a placeholder
   - `[n / Esc]` deny; the agent is told and picks another approach
3. **Permission modes** (`security.permission_mode`, or `/permission`):
   - `ask` (default): prompts for `bash` commands that the built-in risk assessment flags
     (e.g. `rm -rf`, `sudo`, destructive git, firewall changes, access to secrets such as `.env`),
     also when they are wrapped in `bash -c`, `eval`, subshells, `xargs`, or `find -exec`, and for
     code it cannot inspect (`python -c`, a shell reading from stdin). The assessment uses pattern
     rules: a command it does not recognize, such as a script the agent wrote, runs without a
     prompt. Other tools run without prompting. Subagents follow the same mode.
   - `strict` and `review`: prompt for every tool call. (They currently behave the same; `review`
     is the mode intended for using private results.)
   - `trust`: never prompts. Use only in disposable environments.

   Commands in `security.bash_whitelist` skip approval in every mode, except commands the risk
   assessment rates as dangerous. Entries match one simple command word by word (`cargo *` covers
   `cargo test`, never `cargo build && rm -rf ~`); see
   [docs/CONFIGURATION.md](docs/CONFIGURATION.md).
4. **Shell isolation** (`security.sandbox_mode` and `execution.backend`):
   - `host` + `path_jail` (default): commands run as normal host processes in the workspace
     directory. They can read and write anything your user can.
   - `bubblewrap` / `auto`: commands run under bubblewrap on Linux when `bwrap` is installed.
   - `lawang`: commands run in an isolated cell of a separate lawang daemon (`process` =
     bubblewrap, `sandbox` = gVisor, `vm` = Firecracker). One cell per session. Applies to
     sessions run by the daemon; MCP servers still run on the host.
5. **Daemon access.** A daemon started by the CLI always has a token: one is generated when you
   give none and stored in the owner-only `daemon.json`, where local clients find it.
   `maukode server status` prints the web UI address including the token. A foreground
   `maukode server` refuses a non-loopback address without `--token`.
6. **Project trust.** A repository's own `mcp.json` / `.maukode/mcp.json` can name any command,
   so its MCP servers only start after `maukode trust` in that folder (`--revoke` undoes it).
7. **Web fetch.** `fetch_web_content` refuses link-local and cloud-metadata addresses and the
   local daemon API, also through redirects.

---

## 📚 Documentation

- [docs/ROADMAP.md](docs/ROADMAP.md): feature status and backlog (the single source of truth).
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md): `config.json` reference, MCP, web search, files.
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): daemon, agent loop, checkpoints, code map, storage.
- [docs/API.md](docs/API.md): HTTP and SSE API of the daemon.
- [docs/MEMORY_ARCHITECTURE.md](docs/MEMORY_ARCHITECTURE.md) and
  [docs/SUBAGENT_ARCHITECTURE.md](docs/SUBAGENT_ARCHITECTURE.md): subsystem designs.
- [docs/WEB_UI_ROADMAP.md](docs/WEB_UI_ROADMAP.md) and [docs/TUI_ROADMAP.md](docs/TUI_ROADMAP.md):
  product direction for each client.
- [docs/CI_TEST_PLAN.md](docs/CI_TEST_PLAN.md): how tests run in CI and what is still missing.
- [CHANGELOG.md](CHANGELOG.md): release notes.

---

## 📜 License

Licensed under either of:
- [Apache License, Version 2.0](LICENSE-APACHE)
- [MIT License](LICENSE-MIT)

at your option.
