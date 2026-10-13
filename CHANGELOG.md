# Changelog

Semua perubahan penting untuk proyek ini akan didokumentasikan dalam file ini.

Format berdasarkan [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
dan proyek ini mengikuti [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1] - 2026-10-14

Patch release. 1 PR (#72) sejak v0.3.0.

### Fixed

- **Sinkronisasi Session Model**: Memperbaiki issue di mana pemilihan model via
  TUI (`/model`, `ModelSelector` via F2, dan `ProviderSelector` via F3) atau saat
  pembuatan sesi baru tidak tersinkronisasi ke sesi yang tersimpan di SQLite database.
  Akibatnya server `effective_model` tetap terkunci pada model default sesi
  (misal `"auto"`). Sekarang memanggil `set_session_model` (`PUT /api/v1/sessions/{id}/model`)
  dan menyinkronkan status sesi secara konsisten.
- **Propagasi CLI `--max-steps`**: Memperbaiki flag CLI `maukode --max-steps <N>` yang
  sebelumnya terabaikan dan selalu fallback ke default 50 steps pada daemon mode
  dan standalone TUI mode. Sekarang nilai `--max-steps` dikirim ke endpoint
  `POST /api/v1/agent/config` (`client.set_agent_config`) pada daemon mode dan
  diteruskan ke `run_tui` pada standalone mode.
- **Context Window Gateway Routing (`"auto"`)**: Memperbaiki resolusi context window
  estimator untuk alias routing seperti `"auto"` (yang umum digunakan oleh gateway
  seperti LLMKita dengan target model 1M seperti GLM-5.3/Claude/GPT) agar tidak
  fallback ke batas konservatif 128k, melainkan teresolusi ke 1.000.000 tokens (1M).

### Release Process

- Bump versi ke 0.3.1 di seluruh manifest (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, kedua Cargo.lock)

## [0.3.0] - 2026-10-13

Minor release. 1 PR (#69) sejak v0.2.1.

### Added — Post-Turn Reflection Loop

- **Reflection engine** (`crates/maukode-core/src/reflection.rs`): setelah sebuah
  turn selesai, agent mengevaluasi performanya sendiri dan mengekstrak lessons
  yang durable dari percakapan yang telah diselesaikan.
- **`ReflectionConfig`**: opt-in via config dengan parameter:
  - `enabled` — master switch (default: false)
  - `min_tool_calls` — minimum jumlah tool calls dalam turn sebelum reflection
    triggers (default: 3), menghindari refleksi pada turn trivial
  - `skill_creation` — mengizinkan engine menulis skill files baru ke
    `.maukode/skills/learned/` (default: false)
- **Lesson extraction**: parsing output LLM untuk `[memory: title | taxonomy: X,
  content]` blocks, dengan taxonomy extraction (gotchas, conventions, dll)
- **Skill proposal**: parsing `SKILL:` blocks menjadi `ProposedSkill` dengan
  name, description, dan prompt
- **SessionRunner integration**: `with_reflection_config()` builder method
- **CLI flag**: `--reflection` untuk mengaktifkan reflection di TUI
- **Server API**: field `reflection` di `AgentConfig` dan `SetAgentConfigRequest`
  untuk mengatur reflection via REST API
- **Desktop Settings**: UI toggle untuk reflection config di Settings page
- **TS bindings**: `ReflectionConfig.ts` generated untuk desktop app

### Changed

- `AgentConfig` tidak lagi mengimplementasikan `Copy` (menambahkan field
  non-Copy `ReflectionConfig`). Handler server di-update untuk menggunakan
  `.clone()`.
- Workspace version bumped dari 0.2.1 ke 0.3.0

### Release Process

- Bump versi ke 0.3.0 di seluruh manifest (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, kedua Cargo.lock)

## [0.2.1] - 2026-10-13

Patch release. 1 PR (#66) sejak v0.2.0.

### Fixed

- **Compaction context window sizes**: Ganti hardcoded 200K-for-all-Claude dan
  128K default dengan ukuran context window per-model yang akurat, bersumber
  dari dokumentasi provider resmi:
  - **Claude**: 5 family (opus/sonnet/haiku/fable/mythos) + Opus 4.7/4.8 → 1M;
    Opus 4/4.5, Sonnet 4/4.5, Haiku 3.5, 3.x → 200K
  - **GPT**: GPT-6/5.4/5.6 → 1.050.000; GPT-5/5.1 → 400.000; GPT-4o → 128.000;
    GPT-4 → 8.192; GPT-4-32k → 32.768; GPT-3.5 → 16.384
  - **GLM**: GLM-5.x → 1.000.000; GLM-4.x → 131.072
  - Pattern matching mendukung notasi dashed (`claude-opus-4-8`) dan dotted
    (`claude-opus-4.8`)

### Release Process

- Bump versi ke 0.2.1 di seluruh manifest (workspace Cargo.toml, Tauri Cargo.toml,
  tauri.conf.json, package.json, kedua Cargo.lock)
- Tag `v0.2.1` sebelumnya dibuat tanpa release commit — versi masih 0.2.0.
  Perbaikan ini memastikan binary melaporkan versi yang benar.

## [0.2.0] - 2026-10-13

144 commit, 41 PR (#18–#64) sejak v0.1.0. ~27.000 baris kode baru.

### Added — Desktop App (Tauri + React)

- Aplikasi desktop native berbasis Tauri dengan WebView sistem (`apps/desktop/`)
- UI chat-style: sidebar session list, panel chat, composer dengan context
  mentions, token gauge, chips, dan footer affordances
- Migrasi ke Tailwind v4 + shadcn/ui (Radix UI + lucide icons)
- Manajemen session: pencarian, filter status, pinning, archive, rename, model
  switcher per-session
- Command Palette dan Shortcuts modal (navigasi keyboard-first)
- Settings page dengan Radix UI, plugins management, dan flow sign-in ChatGPT
- Review panel: per-file diffs, undo, worktree commit/merge/delete
- Subagent cards interaktif dengan trace drawer slide-over
- Native notifications dan repository picker
- External editor integration
- Project context (branch name, info project) di session list dan header
- Playwright e2e tests untuk desktop UI

### Added — Memory & Knowledge Store

- Crate baru `maukode-memory` dengan 7 modul:
  - Project-scoped knowledge store
  - FTS5 full-text search
  - Memory extraction otomatis dari konteks percakapan
  - Memory links (relasi antar knowledge items)
  - Memory tools (ekspos memory ke agent)
  - Scope & models
  - Error handling khusus

### Added — Security & Sandboxing

- Runtime command sandbox dengan mode: `off`, `path_jail`, `bubblewrap`, `auto`
- MCP server process sandboxing
- API endpoint untuk mengubah mode sandbox runtime
- Private tool review mode (gate akses ke likely secrets behind approval)
- DNS-rebinding Host guard untuk proteksi server

### Added — Subagent System

- SubagentTool implementation (child SessionRunner invocation)
- Subagent profiles: `codebase_researcher`, `security_auditor`
- MCP scoping & tool filtering untuk subagent
- Subagent registry (read-only) untuk discovery
- Server route `/api/v1/subagents` untuk list profiles
- Subagent events masuk ke global event feed

### Added — Skills Engine

- Backend skills engine dengan discovery dari markdown files
- Server endpoints untuk list dan manage skills
- SkillDefinition protocol schema

### Added — Plugin System

- Backend plugin system: discovery, loading, management
- Plugin management API (enable/disable)
- Plugin MCP server config

### Added — Plans, Timeline & Prompt Queuing

- Structured plans: PlanStep, Plan, StepStatus, FinishReason
- Timeline tracking step-by-step
- Prompt queuing & steering (QueueSteeringPrompt)
- Live plan checklist widget di TUI

### Added — Audit System

- Audit provenance viewer di UI
- Permission provenance (record semua permission decisions)
- Compaction provenance (record automatic compaction events)
- Persistent session turn identifiers
- SQLite tables untuk permission dan compaction audit

### Added — Preview System

- Sandboxed preview lifecycle API (#61): start/stop/refresh preview dev-server
- Live dev-server preview panel (#62): reverse proxy untuk preview web app
- PreviewInfo response: status, port, URL, readiness, error

### Added — Worktree & Parallel Agents

- Parallel-agent management dengan git worktrees
- Worktree status: commit/merge/delete dari UI
- Per-session model (setiap session bisa pakai model berbeda)

### Added — ChatGPT Authentication

- Sign in with ChatGPT provider (#18)
- ChatGPT auth flow (`chatgpt_auth.rs`, `chatgpt_client.rs`)
- Login lifecycle: login status, account management, use account

### Added — Server API

- SSE event replay dengan sequence numbers untuk reconnect
- Per-session permissions (permission mode per session, fork-able)
- Session titles (set/get)
- Structured audit trail
- Parallel-agent API: worktree, per-session model, global feed
- DNS-rebinding guard (Host header validation)
- Routes: auth, events, plugins, preview, settings, skills

### Added — CI & Release Infrastructure

- Linux build workflow (`linux-build.yml`): tarball release untuk x86_64,
  smoke test under Xvfb, upload artifact
- Desktop CI checks: compile + smoke-test Tauri shell
- Self-hosted runner (#57)
- Blacksmith build cache
- TS bindings sync via `cargo test --features ts`

### Added — TUI Enhancements

- Markdown rendering dengan pulldown-cmark (`markdown.rs`)
- Render cache (`render_cache.rs`) untuk stop O(history) redraws
- Loading animations
- Turn cancellation (cancel generation mid-stream)
- Idle redraw skip (hemat CPU)
- Subagent trace inspector modal
- Plan checklist widget

### Changed

- Workspace version bumped dari 0.1.0 ke 0.2.0
- Desktop app migrated dari native HTML selects ke Radix UI components
- Server event system menggunakan sequence-numbered SSE replay

### Fixed

- Agent never ends turn silently (push model untuk finish tasks)
- LLM stream tolerance: EOF, flaky SSE streams, retry transient failures
- SQLite lock contention saat first open (flaky CI fix)
- Store cleanup: empty sessions, prevent `read_file` panic (#53)
- Send bound fix: box-pin `start_turn` future untuk ensure `Send`
- Multiple desktop UI fixes: cropping, focus, sidebar toggle, localStorage
  guard, platform helpers
- PreviewRequest TypeScript type: add missing `sandbox_mode` field (#63)
- Tauri Cargo.lock out-of-date untuk `--locked` release build (#64)

## [0.1.0] - 2026-10-03

Initial release.
