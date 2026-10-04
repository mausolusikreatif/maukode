# MauKode Server API (v1)

Base URL `http://127.0.0.1:4096`. All `/api/v1/*` routes except `health` require
`Authorization: Bearer <token>` when the server was started with a token. Errors are plain-text
bodies with a 4xx/5xx status.

## Security model
- **No token (local mode):** browser requests carrying a foreign `Origin` get `403`; CORS only
  allows `localhost`, `127.0.0.1`, `[::1]`, `tauri://localhost` and `tauri.localhost`. Clients that
  send no `Origin` (CLI, native HTTP) are unaffected.
- **Token set:** CORS is open; the bearer token is the only gate.
- Provider API keys are never returned. `GET /providers` gives `api_key: null` and
  `has_api_key`. `POST /providers` with `api_key: null` keeps the stored key.

## Endpoints
| Method | Path | Body / query | Notes |
|---|---|---|---|
| GET | `/health`, `/api/v1/health` | | public |
| GET/POST | `/providers` | POST: `ProviderConfig` | upsert |
| POST | `/providers/active` | `{provider_id}` | |
| GET | `/models?provider=` | | |
| POST | `/models/active` | `{model}` | |
| GET/POST | `/config/agent` | `{max_steps?, max_tokens?}` | `1..=200` / `1..=200000`; persisted |
| GET | `/permissions/mode?session_id=` | | per-session if given |
| POST | `/permissions/mode` | `{mode, session_id?}` | with `session_id` only that session changes; without, default **and all** sessions change |
| GET/POST | `/sessions` | GET `?directory=&all=`; POST `{directory?, title?}` | |
| GET | `/sessions/{id}` | | detail + snapshot (below) |
| PATCH | `/sessions/{id}` | `{title}` | blank/null restores auto title |
| DELETE | `/sessions/{id}` | | cancels running turn, removes data; 404 if unknown |
| POST | `/sessions/{id}/prompt` | `{prompt}` | `409` if a turn is already running |
| POST | `/sessions/{id}/cancel` | | |
| GET | `/sessions/{id}/events` | `?since=` or `Last-Event-ID` | SSE, below |
| POST | `/sessions/{id}/permissions/{request_id}` | `{decision}` | `allow_once \| allow_session \| allow_private \| deny` |
| GET | `/sessions/{id}/audit` | | `{summary, report}`; `report` is structured (latest 200 each) |
| GET | `/sessions/{id}/diff`, `/checkpoints`, `/map`, `/mcp` | map: `?subpath=&query=&max_tokens=` | |
| POST | `/sessions/{id}/undo` | `{turns?}` | |
| POST | `/sessions/{id}/init` | `{force}` | |
| POST | `/sessions/{id}/compact` | `{focus?}` | |

(`/api/v1` prefix omitted above except for health.)

## Session snapshot (`GET /sessions/{id}`)
`{id, title, directory, state, running, last_event_seq, pending_permissions[], messages[], created_at, updated_at}`

Use it on (re)connect: render `messages`, show `pending_permissions` as prompts, then open SSE with
`?since=last_event_seq` so nothing between snapshot and stream is lost or duplicated.

## Events (SSE)
Each message: `id: <seq>` and `data: <LLMEvent JSON>` (`type` tag: `text_delta`, `reasoning_delta`,
`tool_call_start|delta|finish`, `tool_result`, `private_tool_result`, `permission_request`, `usage`,
`context_warning`, `finish`, `error`, …). `seq` is per-session and monotonically increasing.

- Without `since`/`Last-Event-ID` you receive live events only.
- With either, the server first replays logged events with `seq > since`, then goes live. The log is
  in-memory, last 2000 events per session, and is lost on server restart (use the snapshot then).
- A slow consumer is caught up from the log rather than having events dropped.
- Browser `EventSource` sends `Last-Event-ID` automatically on reconnect — but cannot set an
  `Authorization` header, so token mode needs a `fetch`-based SSE reader.
