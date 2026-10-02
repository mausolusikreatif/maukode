# MauKode Configuration Guide ⚙️

This document provides a comprehensive reference for configuring **MauKode** (`~/.config/maukode/config.json`) and integrating external services such as Model Context Protocol (MCP) servers and web search providers.

---

## 📁 File Locations

- **Primary Configuration**: `~/.config/maukode/config.json`
- **Global MCP Configuration**: `~/.config/maukode/mcp.json`
- **Project-Local MCP Configuration**: `<workspace>/.maukode/mcp.json` or `<workspace>/mcp.json`
- **Global Memory Guidelines**: `~/.config/maukode/AGENTS.md` (or `RULES.md`)
- **Project Memory Guidelines**: `<workspace>/AGENTS.md`
- **SQLite Database**: `~/.local/share/maukode/maukode.db`
- **Daemon Metadata**: `~/.local/share/maukode/daemon.json`
- **Daemon Logs**: `~/.local/share/maukode/server.log`

---

## 🛠️ Complete `config.json` Schema

Below is an annotated example of a complete `config.json`:

```json
{
  "active_provider": "anthropic",
  "active_model": "claude-3-7-sonnet-20250219",
  "providers": {
    "anthropic": {
      "name": "Anthropic",
      "provider_type": "anthropic",
      "base_url": "https://api.anthropic.com/v1",
      "api_key": "sk-ant-api03-..."
    },
    "openai": {
      "name": "OpenAI",
      "provider_type": "openai",
      "base_url": "https://api.openai.com/v1",
      "api_key": "sk-proj-..."
    },
    "openrouter": {
      "name": "OpenRouter",
      "provider_type": "openai",
      "base_url": "https://openrouter.ai/api/v1",
      "api_key": "sk-or-v1-..."
    },
    "ollama": {
      "name": "Ollama (Local)",
      "provider_type": "openai",
      "base_url": "http://127.0.0.1:11434/v1",
      "api_key": null
    }
  },
  "security": {
    "permission_mode": "ask",
    "jail_path_traversal": true,
    "bash_whitelist": [
      "cargo test*",
      "cargo check*",
      "git status*",
      "git diff*",
      "git log*",
      "npm test*"
    ]
  },
  "compaction": {
    "auto_compaction": true,
    "warning_threshold": 0.75,
    "critical_threshold": 0.85,
    "prune_tool_results_chars": 2000
  },
  "web_search": {
    "enabled": true,
    "provider": "duckduckgo",
    "api_key": null,
    "endpoint": null,
    "max_results": 8
  },
  "mcp_servers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "ghp_..."
      }
    },
    "fetch": {
      "command": "uvx",
      "args": ["mcp-server-fetch"],
      "env": {}
    }
  }
}
```

---

## 🔒 Security Configuration

The `"security"` block governs sandbox boundaries and human-in-the-loop approvals:

| Field | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `permission_mode` | `string` | `"ask"` | Confirmation policy: `"ask"` prompts on dangerous commands (`rm`, `sudo`, `pkill`, git force actions); `"strict"` prompts on every command; `"trust"` auto-allows all commands (useful in isolated CI containers). |
| `jail_path_traversal` | `bool` | `true` | When `true`, enforces strict workspace confinement. Prevents tools from accessing or modifying paths outside the active working directory. |
| `bash_whitelist` | `array` | `[]` | List of glob patterns for safe bash commands that execute immediately without prompting for human approval. |

---

## 🗜️ Compaction & Token Optimization

The `"compaction"` block manages context window limits and automatic summarization:

| Field | Type | Default | Description |
| :--- | :--- | :--- | :--- |
| `auto_compaction` | `bool` | `true` | Automatically triggers conversation compaction when usage exceeds the `critical_threshold`. |
| `warning_threshold` | `float` | `0.75` | Context usage fraction ($75\%$) at which a warning is flagged in the status bar. |
| `critical_threshold` | `float` | `0.85` | Context usage fraction ($85\%$) at which compaction automatically executes to prevent context overflow. |
| `prune_tool_results_chars`| `integer` | `2000` | Max character length for older tool outputs during pruning. Recent turn outputs are never truncated. |

> **Note on Auditing**: Even when conversations are compacted, the raw, uncompressed message history is permanently archived in SQLite's `messages_audit` table.

---

## 🌐 Web Search Providers

The `"web_search"` block configures real-time documentation retrieval:

### 1. DuckDuckGo (Zero-Config Default)
Works out of the box with zero API keys or external accounts:
```json
"web_search": {
  "enabled": true,
  "provider": "duckduckgo",
  "max_results": 8
}
```

### 2. Brave Search API
```json
"web_search": {
  "enabled": true,
  "provider": "brave",
  "api_key": "YOUR_BRAVE_SEARCH_API_KEY",
  "max_results": 8
}
```

### 3. Tavily AI Search
```json
"web_search": {
  "enabled": true,
  "provider": "tavily",
  "api_key": "tvly-YOUR_KEY",
  "max_results": 8
}
```

### 4. Self-Hosted SearXNG
```json
"web_search": {
  "enabled": true,
  "provider": "searxng",
  "endpoint": "https://searx.example.com",
  "max_results": 8
}
```

---

## 🔌 Model Context Protocol (MCP) Configuration

MauKode supports connecting to external tool servers implementing the Anthropic Model Context Protocol via stdio JSON-RPC 2.0.

### Discovery Precedence
MauKode loads MCP servers in the following cascading order:
1. `config.json` (`mcp_servers` section)
2. Global user file: `~/.config/maukode/mcp.json`
3. Project-local file: `<workspace>/.maukode/mcp.json` or `<workspace>/mcp.json`

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
