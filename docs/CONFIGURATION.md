# MauKode Configuration Guide ⚙️

This document provides a comprehensive reference for configuring **MauKode** (`~/.config/maukode/config.json`) and integrating external services such as Model Context Protocol (MCP) servers and web search providers.

---

## 📁 File Locations

Paths below are for Linux; other platforms use their standard config and data directories. Setting
`MAUKODE_HOME` moves everything to `$MAUKODE_HOME/config` and `$MAUKODE_HOME/data`.

- **Primary configuration**: `~/.config/maukode/config.json`
- **ChatGPT plan credentials**: `~/.config/maukode/chatgpt_auth.json`
- **Global MCP configuration**: `~/.config/maukode/mcp.json` (or `~/.maukode/mcp.json`)
- **Project MCP configuration**: `<workspace>/.maukode/mcp.json` or `<workspace>/mcp.json`
- **Global instructions**: `~/.config/maukode/AGENTS.md` (or `RULES.md`)
- **Project instructions**: `<workspace>/AGENTS.md`
- **Plugins**: `~/.config/maukode/plugins/`
- **SQLite database** (sessions, audits, knowledge store): `~/.local/share/maukode/maukode.db`
- **Shadow checkpoints**: `~/.local/share/maukode/shadow_git/`
- **Daemon metadata**: `~/.local/share/maukode/daemon.json`
- **Daemon log**: `~/.local/share/maukode/server.log`

---

## 🛠️ `config.json` Schema

An annotated example. Every block except `active_provider`, `active_model`, and `providers` is
optional and falls back to the defaults shown. Most fields can also be changed from the web UI
Settings or the TUI.

```json
{
  "active_provider": "anthropic",
  "active_model": "claude-sonnet-5-5",
  "providers": [
    {
      "id": "anthropic",
      "name": "Anthropic",
      "kind": "anthropic",
      "base_url": "https://api.anthropic.com",
      "api_key": null,
      "default_model": null
    },
    {
      "id": "openrouter",
      "name": "OpenRouter",
      "kind": "open_ai_compatible",
      "base_url": "https://openrouter.ai/api",
      "api_key": null,
      "default_model": null
    },
    {
      "id": "ollama",
      "name": "Ollama (Local)",
      "kind": "open_ai_compatible",
      "base_url": "http://localhost:11434",
      "api_key": null,
      "default_model": null
    }
  ],
  "security": {
    "permission_mode": "ask",
    "jail_path_traversal": true,
    "allowed_external_paths": [],
    "sandbox_mode": "path_jail",
    "bash_whitelist": ["cargo *", "git status", "git diff", "npm test"]
  },
  "execution": {
    "backend": "host",
    "lawang": {
      "url": "http://127.0.0.1:7777",
      "token": null,
      "isolation": "process",
      "image": null,
      "memory_mb": null
    }
  },
  "agent": {
    "max_steps": 2000,
    "max_tokens": 8192,
    "reflection": { "enabled": false, "min_tool_calls": 3, "skill_creation": true },
    "consolidation": {
      "enabled": false,
      "min_memories": 20,
      "threshold": 0.5,
      "batch_size": 10,
      "abstraction": false,
      "abstraction_min_memories": 30
    }
  },
  "web_search": {
    "enabled": true,
    "provider": "duckduckgo",
    "api_key": null,
    "endpoint": null,
    "max_results": 5
  },
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_..." }
    }
  }
}
```

### Providers

`providers` is a list. `kind` is one of:

- `anthropic`: the Anthropic Messages API.
- `open_ai_compatible`: any OpenAI-compatible Chat Completions endpoint (OpenAI, OpenRouter, Ollama,
  DeepSeek, llm-kita, …). `base_url` is the server root; the adapter adds the API path.
- `chat_gpt`: a ChatGPT plan via Sign in with ChatGPT (`maukode auth login`); no API key.

The defaults include `llmkita` (the default active provider), `anthropic`, `openai`, `chatgpt`,
`ollama`, and `openrouter`. When `api_key` is `null`, the key is read from the environment (see
below).

---

## 🔒 Security

| Field | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `permission_mode` | `string` | `"ask"` | `"ask"` prompts for `bash` commands that the risk assessment flags (`rm -rf`, `sudo`, destructive git, secret access, …); unknown commands and other tools run without a prompt. `"strict"` and `"review"` prompt for every tool call (they currently behave the same). `"trust"` never prompts. Change at runtime with `/permission <mode>`. |
| `jail_path_traversal` | `bool` | `true` | Confine file tools (`read_file`, `write_file`, `edit_file`, `glob`, `grep`) to the workspace. Does not affect `bash`. |
| `allowed_external_paths` | `array` | `[]` | Extra directories the file tools may access outside the workspace. |
| `sandbox_mode` | `string` | `"path_jail"` | How `bash` runs on the host backend: `"path_jail"` or `"off"` (an ordinary host process), `"bubblewrap"` (Linux, needs `bwrap`), or `"auto"` (bubblewrap when available, otherwise `"path_jail"`). |
| `bash_whitelist` | `array` | `["cargo *", "git status", "git diff", "npm test"]` | Commands that never need approval, in any mode. An entry ending in `*` matches any command that **starts with** the text before it; other entries must match exactly. Prefix matching also covers chained commands (`cargo build && …`), so keep entries narrow. |

## 📦 Execution Backend

`execution` chooses where `bash` commands run:

| Field | Default | Description |
| :--- | :--- | :--- |
| `backend` | `"host"` | `"host"`: local processes, using `security.sandbox_mode`. `"lawang"`: an isolated cell of a lawang daemon, one per session. |
| `lawang.url` | `"http://127.0.0.1:7777"` | URL of the lawang daemon (`http://` or `https://`). |
| `lawang.token` | `null` | Bearer token of the lawang daemon, if it uses one. |
| `lawang.isolation` | `"process"` | `"process"` (bubblewrap), `"sandbox"` (gVisor), or `"vm"` (Firecracker). |
| `lawang.image` | `null` | Root filesystem image such as `alpine:3.20`. Required for `"vm"`. |
| `lawang.memory_mb` | `null` | Memory limit of the cell in MiB (at least 1). |

The project folder is mounted at `/work` in the cell; for a VM it is copied in and back. File tools
keep working on the host folder. The setting applies to sessions run by the daemon (web UI and
API); MCP servers always run on the host, and the CLI/TUI in-process paths do not use the lawang
backend yet. If the cell cannot be used, commands fail with an explanation; they never fall back
to the host silently.

## 🤖 Agent

| Field | Default | Description |
| :--- | :--- | :--- |
| `max_steps` | `2000` | Maximum model steps per turn. |
| `max_tokens` | `8192` | Output token limit per model call. A cut-off answer is continued automatically up to twice. |
| `reflection.enabled` | `false` | After a turn, let the agent evaluate its work and record lessons in the knowledge store. |
| `reflection.min_tool_calls` | `3` | Only reflect on turns with at least this many tool calls. |
| `reflection.skill_creation` | `true` | Allow reflection to propose skills. |
| `consolidation.enabled` | `false` | After a turn, merge near-duplicate memories. |
| `consolidation.min_memories` | `20` | Minimum number of memories before consolidation runs. |
| `consolidation.threshold` | `0.5` | Similarity (Jaccard) at which two memories count as duplicates. |
| `consolidation.batch_size` | `10` | Pairs sent to the model per batch. |
| `consolidation.abstraction` | `false` | Also synthesize higher-level summaries. |
| `consolidation.abstraction_min_memories` | `30` | Minimum memories before abstraction runs. |

Context compaction is not configurable: older tool outputs are pruned to 2,000 characters, a
warning appears at 75% of the model's context window, and history is compacted automatically at
85% (or with `/compact`). The uncompacted history stays in the `messages_audit` table.

---

## 🔐 Environment Variables and Secret Files

MauKode does not load `.env` files automatically. Provider credentials are read from
process environment variables only when the corresponding provider is configured;
values in the explicit JSON configuration take precedence over environment fallbacks. The built-in
providers use `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `OPENROUTER_API_KEY`, and
`LLMKITA_API_KEY`. Environment variables are inherited by local subprocesses, including Bash tools
and configured MCP servers, but are never intentionally inserted into model messages.

In the default `ask` permission mode, commands that appear to inspect secret-bearing
files or variables require approval. This includes `.env*`, credential/secret files,
private keys, `printenv`/`env`, and common API-key or access-token references. Use
`review` when every tool call should be reviewed. `trust` disables these safeguards
and should only be used in an isolated environment.

Do not put secrets in prompts, tool arguments, checked-in configuration, or project
files. If a tool output contains a secret, reject it or use private execution (`p`)
so the result is withheld from the model and replaced with a placeholder in history.

Other environment variables:

| Variable | Effect |
| :--- | :--- |
| `MAUKODE_HOME` | Relocate config and data directories (used by tests and portable installs). |
| `MAUKODE_AUTH_TOKEN` | Bearer token for `maukode server` (passed to the daemon via the environment, not argv). |
| `MAUKODE_WEB_DIR` | Directory of the built web UI for `maukode server`. |
| `MAUKODE_MEMORY_AUTO_EXTRACT` | `1`/`true` enables automatic end-of-turn memory extraction. |

---

## 🌐 Web Search Providers

The `"web_search"` block configures real-time documentation retrieval:

### 1. DuckDuckGo (Zero-Config Default)
Works out of the box with zero API keys or external accounts:
```json
"web_search": {
  "enabled": true,
  "provider": "duckduckgo",
  "max_results": 5
}
```

### 2. Brave Search API
```json
"web_search": {
  "enabled": true,
  "provider": "brave",
  "api_key": "YOUR_BRAVE_SEARCH_API_KEY",
  "max_results": 5
}
```

### 3. Tavily AI Search
```json
"web_search": {
  "enabled": true,
  "provider": "tavily",
  "api_key": "tvly-YOUR_KEY",
  "max_results": 5
}
```

### 4. Self-Hosted SearXNG
```json
"web_search": {
  "enabled": true,
  "provider": "searxng",
  "endpoint": "https://searx.example.com",
  "max_results": 5
}
```

---

## 🔌 Model Context Protocol (MCP) Configuration

MauKode supports connecting to external tool servers implementing the Anthropic Model Context Protocol via stdio JSON-RPC 2.0.

### Discovery Precedence
MauKode loads MCP servers in the following cascading order:
1. `config.json` (`mcpServers` section; `mcp_servers` is accepted too)
2. Global user file: `~/.config/maukode/mcp.json` or `~/.maukode/mcp.json`
3. Project-local file: `<workspace>/.maukode/mcp.json` or `<workspace>/mcp.json`

Later sources override earlier ones when they define a server with the same name.

### Standalone `mcp.json` Example:
```json
{
  "mcpServers": {
    "sqlite": {
      "command": "uvx",
      "args": ["mcp-server-sqlite", "--db-path", "./test.db"]
    },
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/allowed-dir"]
    }
  }
}
```

To verify loaded servers and their registered tools during a session, use the `/mcp` slash command in the TUI or CLI.
