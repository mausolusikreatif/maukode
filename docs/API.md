# MauKode Server API (v1)

Base URL `http://127.0.0.1:4096`. All `/api/v1/*` routes except `health` require
`Authorization: Bearer <token>` when the server was started with a token. Errors are plain-text
bodies with a 4xx/5xx status.

## Security model
- **Daemons started by the CLI always have a token.** `maukode` / `maukode server start` generate
  one when none is given and store it in the owner-only `daemon.json`; local clients read it from
  there, and `maukode server status` prints the web UI address with `#token=…`.
- **No token (foreground `maukode server` on loopback only):** browser requests carrying a foreign
  `Origin` get `403`, and requests whose `Host` is not a loopback name get `403` (DNS-rebinding
  guard); CORS only allows `localhost`, `127.0.0.1` and `[::1]`. Clients that send no `Origin`
  (CLI, native HTTP) are unaffected. A non-loopback bind address without a token is refused at
  startup, and an empty token counts as no token.
- **Token set:** CORS is open; the bearer token is the only gate. It is compared in constant
  time.
- Provider API keys are never returned. `GET /providers` gives `api_key: null` and
  `has_api_key`. `POST /providers` with `api_key: null` keeps the stored key.

## Endpoints
| Method | Path | Body / query | Notes |
|---|---|---|---|
| GET | `/health`, `/api/v1/health` | | public |
| GET/POST | `/providers` | POST: `ProviderConfig` | upsert; `api_key`: `null` keeps the stored key, `""` removes it |
| DELETE | `/providers/{id}` | | `409` for the active provider, `404` if unknown |
| GET/PUT | `/config/security` | PUT `{permission_mode}` | GET also returns jail/allowed paths/bash whitelist (read-only); PUT persists the default approval mode for **new** sessions |
| GET | `/mcp` | `?directory=` | configured MCP servers; **never starts one** (`not_started` until an agent connects it). Read-only: they run commands, so they are configured in files |
| GET | `/auth/chatgpt` | | `{email, accounts[]}`; never tokens |
| POST | `/auth/chatgpt/login` | `{new_account?, open_browser?}` | starts Sign in with ChatGPT → `{attempt_id, url, browser_opened}`. The loopback callback listener lives in the daemon, so the approving browser must run on the **same machine** |
| GET/DELETE | `/auth/chatgpt/login/{attempt_id}` | | poll `{state: pending\|succeeded\|failed, email?, error?}` / cancel (closes the listener) |
| POST | `/auth/chatgpt/account` | `{client_id}` | switch account |
| DELETE | `/auth/chatgpt` | | revoke and sign out |
| POST | `/providers/active` | `{provider_id}` | |
| GET | `/models?provider=` | | |
| POST | `/models/active` | `{model}` | |
| GET/POST | `/config/agent` | `{max_steps?, max_tokens?}` | `1..=10000` / `1..=200000`; persisted; defaults 2000 steps / 8192 tokens |
| GET | `/permissions/mode?session_id=` | | per-session if given |
| POST | `/permissions/mode` | `{mode, session_id?}` | with `session_id` only that session changes; without, default **and all** sessions change |
| GET | `/sessions` | `?directory=&all=` | items add `running`, `pending_permission_count`, `project_dir`, `worktree_branch`; header `X-Event-Seq` (see Global feed) |
| POST | `/sessions` | `{directory?, title?, provider?, model?, isolation?, chat?}` | `isolation: "worktree"` → own git worktree + branch; `400` if not a git repo / no commits. `chat: true` → chat-only session in a private empty folder (`<data_dir>/maukode/chats/<id>`) with only the web and memory tools; `400` if combined with `directory` or `isolation`; `SessionListItem.chat` marks such sessions |
| GET | `/sessions/{id}` | `?limit=N` (newest N messages) | detail + snapshot (below) |
| GET | `/sessions/{id}/messages` | `?before=&limit=` | backwards paging, default 50, max 500: `{total, start, end, has_more, messages}`; next page: `before=start` |
| PUT | `/sessions/{id}/model` | `{provider_id?, model}` | pins provider/model to this session only |
| GET | `/events` | `?since=` / `Last-Event-ID` | global SSE feed |
| PATCH | `/sessions/{id}` | `{title}` | blank/null restores auto title |
| PUT | `/sessions/{id}/archive` | `{archived}` | list items carry `archived`; archived sessions are still listed (clients hide them) |
| POST | `/sessions/{id}/open` | `{path, line?}` | opens a workspace file with `editor.command` on the daemon's machine; `400` outside the workspace, `404` if missing |
| DELETE | `/sessions/{id}` | `?cleanup_worktree=true&delete_branch=false` | cancels running turn, removes data; removes the worktree checkout (branch kept unless `delete_branch`); 404 if unknown |
| POST | `/sessions/{id}/prompt` | `{prompt, images?}` | `images`: `[{media_type, data}]`, base64 PNG/JPEG/GIF/WebP, at most 5 MB each and 20 per prompt (`400` otherwise); `409` if a turn is already running (the queue holds text only) |
| POST | `/sessions/{id}/cancel` | | |
| GET | `/sessions/{id}/events` | `?since=` or `Last-Event-ID` | SSE, below |
| POST | `/sessions/{id}/permissions/{request_id}` | `{decision}` | `allow_once \| allow_session \| allow_private \| deny` |
| GET | `/sessions/{id}/audit` | | `{summary, report}`; `report` is structured (latest 200 each) |
| GET | `/sessions/{id}/system-prompt` | | the system prompt and tool schemas of the session's latest request: `{hash, created_at, system_prompt, system_prompt_tokens, tool_schema_tokens, tools:[{name,tokens}]}` (token counts are estimates, 4 characters each); `404` before the first request |
| GET | `/sessions/{id}/diff`, `/checkpoints`, `/map`, `/mcp` (non-spawning) | map: `?subpath=&query=&max_tokens=` | diff/checkpoints are computed in the session's own workspace and survive daemon restarts |
| GET | `/sessions/{id}/diff/files` | `?turn=` | `{files:[{path,status,insertions,deletions,binary}]}` |
| GET | `/sessions/{id}/diff/file` | `?path=&turn=` | unified diff of one file (literal path, capped at 1 MiB, `truncated` flag) |
| POST | `/sessions/{id}/undo` | `{turns?}` | `409` while a turn is running |
| GET | `/sessions/{id}/worktree` | | `{project_dir, path, branch, base_commit, dirty, commits_ahead}`; 404 if none |
| POST | `/sessions/{id}/worktree/commit` | `{message?}` | commits all changes on the session branch → `{commit}` |
| POST | `/sessions/{id}/worktree/merge` | `{message?}` | commits, then `--no-ff` merge into the project's current branch → `{result: merged\|nothing_to_merge\|conflict, ...}`; `409` if running or the project has modified tracked files; conflicts are rolled back |
| POST | `/sessions/{id}/init` | `{force}` | |
| POST | `/sessions/{id}/compact` | `{focus?}` | |
| POST | `/sessions/{id}/preview` | `{port?, sandbox_mode?}` | starts a sandboxed `npm run dev` (or `npm start`) in the session's directory; returns `PreviewInfo` |
| GET | `/sessions/{id}/preview` | | returns current `PreviewInfo` (status, port, proxy URL, ready) |
| DELETE | `/sessions/{id}/preview` | | kills the dev server process |
| ANY | `/sessions/{id}/preview/proxy/{*path}` | | reverse-proxy to the dev server; streams body, strips `X-Frame-Options`/`CSP` for iframe embedding, rewrites redirects |

(`/api/v1` prefix omitted above except for health.)

## Parallel agents and worktrees
`isolation: "worktree"` creates `<data_dir>/maukode/worktrees/<session_id>` on branch
`maukode/<id8>` from the project's current `HEAD`. Uncommitted project changes are **not** copied.
The session's `directory` is the worktree; `project_dir` is the original repo (listing by the repo
directory includes its worktree sessions). Sessions without isolation share the checkout and can
conflict with each other — the server does not serialize them.

## Global feed (`GET /events`)
SSE of `GlobalEvent` (`type`: `session_created`, `session_updated`, `session_deleted`,
`turn_started`, `turn_finished {reason, message}`, `permission_requested {request}`,
`permission_resolved`), each with `session_id`; same `id`/replay rules as session events.
Bootstrap a dashboard with `GET /sessions?all=true`, then connect with `?since=<X-Event-Seq>`.

## Persistence
SQLite runs in WAL mode with a 5 s busy timeout; session saves are transactional. The agent's
progress is checkpointed to disk at most every 2 s during a turn and at its end. A session found
mid-turn after a crash loads as `interrupted`.

## Session snapshot (`GET /sessions/{id}`)
`{id, title, directory, state, running, last_event_seq, pending_permissions[], provider, model, worktree, message_total, messages_start, messages[], created_at, updated_at, queued_prompts[], usage}`

Message parts (`type`): `text`, `reasoning`, `tool_call`, `tool_result`, `image` (`media_type`,
base64 `data`), `context` (date and recalled memories MauKode added to a prompt; sent to the model,
not meant for display), and `provider_item` (provider data kept verbatim, such as Anthropic
thinking blocks with their signature).

Use it on (re)connect: render `messages`, show `pending_permissions` as prompts, then open SSE with
`?since=last_event_seq` so nothing between snapshot and stream is lost or duplicated.

## Events (SSE)
Each message: `id: <seq>` and `data: <LLMEvent JSON>` (`type` tag: `text_delta`, `reasoning_delta`,
`tool_call_start|delta|finish`, `tool_result`, `private_tool_result`, `permission_request`, `usage`,
`session_usage`, `context_warning`, `finish`, `error`, …). `seq` is per-session and monotonically
increasing. `usage` describes one provider response (`input_tokens` includes cached tokens);
`session_usage` follows it with the session's totals (subagents included) and `cost_microusd`, the
cost of the responses whose model has a known price (`unpriced_responses` counts the others). The
snapshot's `usage` field holds the same totals.

- **A turn is over only at `turn_stopped {reason, message}`** (always the last event of a turn).
  `finish` events are per provider response — a turn has many — and `error` precedes
  `turn_stopped {reason: "error"}`. `reason`: `completed`, `max_tokens` (output cut off, auto-continue
  exhausted), `max_steps` (step limit hit), `cancelled`, `error`. For `max_tokens`/`max_steps` the
  task is likely unfinished: show `message` and offer "Continue". The same reason arrives on the
  global feed as `turn_finished`.
- Responses cut off by the output limit are resumed automatically (up to 2 times) before stopping.
- Without `since`/`Last-Event-ID` you receive live events only.
- With either, the server first replays logged events with `seq > since`, then goes live. The log is
  in-memory, last 2000 events per session, and is lost on server restart (use the snapshot then).
- A slow consumer is caught up from the log rather than having events dropped.
- Browser `EventSource` sends `Last-Event-ID` automatically on reconnect — but cannot set an
  `Authorization` header, so token mode needs a `fetch`-based SSE reader.


`POST /sessions/{id}/mcp` connects (spawns) the session's configured MCP servers and returns their status.

**Tokenless mode:** requests with a non-loopback `Origin`, or (on loopback binds) a non-loopback `Host`, get 403 (CSRF / DNS-rebinding guard).
