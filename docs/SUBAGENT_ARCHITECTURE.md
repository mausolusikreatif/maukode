# Subagent Architecture & Execution Model 🤖

This document outlines the architecture, execution lifecycle, security isolation, protocol schemas, and user interface specifications for **Subagents** in MauKode.

---

## 1. Vision & Core Principles

MauKode supports autonomous multi-agent decomposition to solve complex software engineering challenges without degrading the parent conversation context or compromising execution safety.

```text
                               ┌────────────────────────────────┐
                               │     Parent Agent Session       │
                               │  (Full Tools, Workspace Jail)  │
                               └───────────────┬────────────────┘
                                               │
                           invoke_subagent(...)│ (Tool Call)
                                               ▼
         ┌───────────────────────────────────────────────────────────────────────────┐
         │                         Subagent Orchestration Hub                        │
         │                        (Daemon: maukode-server)                           │
         └─────────────┬─────────────────────────────┬───────────────────────────────┘
                       │                             │
         spawn child   ▼                             ▼ spawn child
┌────────────────────────────────────────┐ ┌────────────────────────────────────────┐
│     Codebase Researcher Subagent       │ │        Security Auditor Subagent       │
│  - Tool Registry: Read-Only Jail       │ │  - Tool Registry: Read-Only + AST      │
│  - System Prompt: Research Specialist  │ │  - System Prompt: Vulnerability Hunter │
│  - Fresh Context (0 / 32k tokens)      │ │  - Fresh Context (0 / 32k tokens)      │
│  - No write/edit/bash mutation         │ │  - Sandboxed execution                 │
└──────────────────┬─────────────────────┘ └──────────────────┬─────────────────────┘
                   │                                          │
                   ▼ (Synthesized Findings)                   ▼ (Security Report)
         ┌───────────────────────────────────────────────────────────────────────────┐
         │                  Parent Session Receives ToolResult                       │
         │         (Compact summary: ~500 tokens, zero context degradation)          │
         └───────────────────────────────────────────────────────────────────────────┘
```

### Core Invariants

1. **Backend / Daemon Native**: Subagent orchestration lives entirely in `maukode-core` and `maukode-server`. Neither the Desktop app nor the TUI runs agent logic locally; both connect to daemon SSE event streams.
2. **Context Window Isolation**: Each subagent executes with a fresh, dedicated context window. Raw exploratory outputs (hundreds of grep lines, dozens of file reads) never pollute the parent conversation context.
3. **Hardware & Token Budgets**: Each subagent operates within strict token budgets (e.g., max 32k tokens, max 15 tool iterations) to prevent runaway loops.
4. **Physical Safety Boundaries**: Read-only subagents (e.g., `researcher`, `auditor`) receive an immutable `ToolRegistry` (`create_readonly_registry`) where mutating tools (`edit_file`, `write_file`, mutating `bash`) are physically absent.
5. **Hierarchical Observability**: Subagent execution streams fine-grained lifecycle events over SSE, allowing clients (both Desktop and TUI) to render nested progress indicators without mixing child text into the parent response.

---

## 2. Protocol Schemas (`maukode-protocol`)

Subagents introduce first-class definitions into `maukode-protocol`:

### 2.1 Subagent Profile Specification

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SubagentProfile {
    pub name: String,               // e.g., "codebase-researcher"
    pub display_name: String,       // e.g., "Codebase Researcher"
    pub description: String,        // Tool docstring exposed to the parent LLM
    pub role_prompt: String,        // System prompt steering the specialist
    pub tool_mode: SubagentToolMode,// ReadOnly, Standard, or Custom
    pub max_turns: usize,           // Loop limit (default: 10-15 turns)
    pub max_tokens: Option<usize>,  // Token budget cap
    pub model_tier: ModelTier,      // Inherit, Fast (flash/haiku), or Strong (pro/opus)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubagentToolMode {
    ReadOnly,                       // read_file, grep, glob, get_codebase_map, web_search, fetch_web_content
    WorkspaceMutating,              // Full standard tools (subject to workspace jail & permissions)
    Custom(Vec<String>),            // Explicit list of allowed tool names
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ModelTier {
    Inherit,
    Fast,
    Strong,
}
```

### 2.2 Streaming Protocol Events (`LLMEvent`)

To stream child agent activity to clients in real-time, `LLMEvent` is extended:

```rust
pub enum LLMEvent {
    // ... existing variants (TextDelta, ToolCallStarted, PlanUpdate, etc.)

    /// Emitted when a parent agent invokes a subagent
    SubagentSpawned {
        subagent_id: String,
        parent_session_id: SessionId,
        profile_name: String,
        goal: String,
    },

    /// Emitted for granular subagent activity (tool calls, thinking steps)
    SubagentProgress {
        subagent_id: String,
        step: usize,
        action: String,          // e.g., "Reading crates/maukode-core/src/runner.rs"
        tokens_used: usize,
    },

    /// Emitted when a subagent finishes and returns its synthesized result
    SubagentFinished {
        subagent_id: String,
        status: SubagentStatus,  // Completed, Failed, BudgetExceeded
        summary: String,
        tokens_used: usize,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SubagentStatus {
    Completed,
    Failed(String),
    BudgetExceeded,
    Cancelled,
}
```

---

## 3. Server & Core Execution Engine

### 3.1 Subagent Tool Integration (`SubagentTool`)

Subagent dispatch is exposed to the parent LLM as a native tool:

```json
{
  "name": "invoke_subagent",
  "description": "Delegate a research, audit, or exploration subtask to a specialized child subagent with an isolated context window.",
  "parameters": {
    "type": "object",
    "properties": {
      "profile": {
        "type": "string",
        "enum": ["codebase-researcher", "security-auditor", "doc-writer"],
        "description": "The specialized subagent profile to deploy."
      },
      "goal": {
        "type": "string",
        "description": "Clear, actionable objective and required deliverables for the subagent."
      },
      "context_hints": {
        "type": "array",
        "items": { "type": "string" },
        "description": "Optional list of relevant files, symbols, or error messages to prime the subagent."
      }
    },
    "required": ["profile", "goal"]
  }
}
```

### 3.2 Execution Loop & Lifecycle

When `invoke_subagent` executes:

1. **Child Session Instantiation**:
   - Creates a child `Session` in SQLite marked with `parent_session_id`.
   - Generates a fresh conversation context:
     - Injects `profile.role_prompt`.
     - Injects project `AGENTS.md` and repository skeleton map (`repo_map`).
     - Appends the parent's `goal` and `context_hints` as the child user message.
2. **Tool Boundary Hardening**:
   - If `profile.tool_mode == SubagentToolMode::ReadOnly`:
     - Uses `maukode_tools::create_readonly_registry(jail)`.
     - Only `read_file`, `grep`, `glob`, `web_search`, `fetch_web_content`, and `get_codebase_map` are available. Mutating file tools and unrestricted bash are physically absent.
3. **Child Autonomous Drain**:
   - The child `SessionRunner` executes its own drain loop.
   - At each step, progress events are published to the session event bus as `LLMEvent::SubagentProgress`.
4. **Synthesis & Return**:
   - The child's final assistant text is extracted as the concise summary.
   - Returned to the parent agent as a standard `ToolResult`.

---

## 4. REST Endpoints for Subagents

The Axum daemon (`maukode-server`) exposes endpoints for subagent discovery and inspection:

- `GET /api/subagents/profiles`  
  List available subagent definitions (built-in and workspace-defined).
- `GET /api/sessions/{id}/subagents`  
  Retrieve all child subagents spawned by session `{id}` with status, token usage, and summaries.
- `GET /api/sessions/{id}/subagents/{subagent_id}/transcript`  
  Retrieve the complete audit trail and message transcript of a child subagent.
- `POST /api/sessions/{id}/subagents/{subagent_id}/cancel`  
  Cancel a currently executing child subagent.

---

## 5. Client Interface Specifications

### 5.1 Desktop Interface (`apps/desktop`)

- **Subagent Card in Message Stream**:
  Displays an expandable component showing child agent avatar, role, goal, live tool activity, and token count.
- **Child Trace Drawer**:
  Clicking on the subagent card opens a slide-over panel displaying the full step-by-step thinking process, tool parameters, and raw outputs of the subagent without cluttering the primary conversation.

### 5.2 Terminal UI (`crates/maukode-tui`)

- **Compact In-Stream Card**:
  Renders a high-density box in the chat scrollback displaying live status (`[◉] grep "token_refresh" ... 3.2k tokens`).
- **Trace Inspector Modal (`ActiveModal::SubagentViewer`)**:
  Pressing `Enter` on a subagent item or triggering `/subagents` opens an interactive viewer to inspect child turns, filter logs, and view tool diffs.
- **Multi-Subagent Tabs**:
  When multiple subagents run concurrently, tab keys (`1`-`9` or `h`/`l`) toggle between child streams.

---

## 6. Implementation Phases

| Phase | Scope | Components |
| :--- | :--- | :--- |
| **P1: Schemas & Tool Registry** | Subagent profiles, read-only tool registry, protocol events | `maukode-protocol`, `maukode-tools` |
| **P2: Core Runner & Invocation** | Child session runner, `invoke_subagent` tool, token budgeting | `maukode-core` |
| **P3: Server REST & Event Streaming** | Axum subagent routes, child event forwarding over SSE | `maukode-server` |
| **P4: TUI Subagent Experience** | In-stream cards, trace viewer modal, subagent hotkeys | `maukode-tui` |
| **P5: Desktop Subagent Experience** | Interactive timeline nodes, trace drawer, child logs | `apps/desktop` |
