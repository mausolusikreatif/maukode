import { SseParser } from "./sse";
import type {
  AgentConfig,
  AuditResponse,
  ChatGptLoginStarted,
  ChatGptLoginStatus,
  ChatGptStatus,
  Checkpoint,
  CodebaseMapResponse,
  CreateSessionRequest,
  CreateSessionResponse,
  FileDiffListResponse,
  FileDiffResponse,
  McpListResponse,
  MergeOutcome,
  MessagesPage,
  ModelsListResponse,
  PermissionDecision,
  PermissionMode,
  PluginManifest,
  PreviewInfo,
  PreviewRequest,
  PromptImage,
  ProviderConfig,
  ProvidersListResponse,
  ExecutionSettings,
  ExecutionStatus,
  SecuritySettings,
  SetExecutionRequest,
  SessionDetailResponse,
  SessionListItem,
  SetAgentConfigRequest,
  SkillDefinition,
  UndoResponse,
  WorktreeStatus,
} from "./types";

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
  }
}

export interface StreamHandlers<T> {
  /** Called for every event with its sequence number (the SSE `id`). */
  onEvent: (event: T, seq: number | undefined) => void;
  /** Called when the connection state changes. */
  onStatus?: (status: "connected" | "reconnecting") => void;
  signal: AbortSignal;
  /** Resume point: events with a greater sequence number are replayed first. */
  since?: number;
}

const enc = (v: string) => encodeURIComponent(v);

export class ApiClient {
  constructor(
    public readonly baseUrl: string,
    private readonly token?: string,
  ) {}

  private headers(extra?: Record<string, string>): Record<string, string> {
    const h: Record<string, string> = { ...extra };
    if (this.token) h.Authorization = `Bearer ${this.token}`;
    return h;
  }

  /** Low-level typed request for endpoints without a dedicated method. */
  async request<T>(method: string, path: string, body?: unknown): Promise<T> {
    const res = await fetch(`${this.baseUrl}${path}`, {
      method,
      headers: this.headers(body === undefined ? undefined : { "Content-Type": "application/json" }),
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    if (!res.ok) throw new ApiError(res.status, (await res.text()) || res.statusText);
    const text = await res.text();
    return (text ? JSON.parse(text) : undefined) as T;
  }

  async health(): Promise<boolean> {
    try {
      const res = await fetch(`${this.baseUrl}/api/v1/health`);
      return res.ok;
    } catch {
      return false;
    }
  }

  /** Lists sessions and the global event sequence to resume the feed from. */
  async listSessions(all = true): Promise<{ sessions: SessionListItem[]; eventSeq: number }> {
    const res = await fetch(`${this.baseUrl}/api/v1/sessions?all=${all}`, { headers: this.headers() });
    if (!res.ok) throw new ApiError(res.status, await res.text());
    return {
      sessions: (await res.json()) as SessionListItem[],
      eventSeq: Number(res.headers.get("x-event-seq") ?? 0),
    };
  }

  createSession(req: CreateSessionRequest) {
    return this.request<CreateSessionResponse>("POST", "/api/v1/sessions", req);
  }
  getSession(id: string, limit?: number) {
    return this.request<SessionDetailResponse>(
      "GET",
      `/api/v1/sessions/${id}${limit ? `?limit=${limit}` : ""}`,
    );
  }
  messages(id: string, before?: number, limit = 50) {
    const q = `limit=${limit}${before === undefined ? "" : `&before=${before}`}`;
    return this.request<MessagesPage>("GET", `/api/v1/sessions/${id}/messages?${q}`);
  }
  deleteSession(id: string, opts: { cleanupWorktree?: boolean; deleteBranch?: boolean } = {}) {
    const q = new URLSearchParams();
    if (opts.cleanupWorktree !== undefined) q.set("cleanup_worktree", String(opts.cleanupWorktree));
    if (opts.deleteBranch !== undefined) q.set("delete_branch", String(opts.deleteBranch));
    const qs = q.toString();
    return this.request<void>("DELETE", `/api/v1/sessions/${id}${qs ? `?${qs}` : ""}`);
  }
  renameSession(id: string, title: string | null) {
    return this.request<void>("PATCH", `/api/v1/sessions/${id}`, { title });
  }
  prompt(id: string, prompt: string, images: PromptImage[] = []) {
    return this.request<void>("POST", `/api/v1/sessions/${id}/prompt`, { prompt, images });
  }
  queuePrompt(id: string, prompt: string) {
    return this.request<{ status: string; session_id: string; queue_len: number }>(
      "POST",
      `/api/v1/sessions/${id}/queue`,
      { prompt },
    );
  }
  steerPrompt(id: string, prompt: string) {
    return this.request<{ status: string; session_id: string }>(
      "POST",
      `/api/v1/sessions/${id}/steer`,
      { prompt },
    );
  }
  getQueue(id: string) {
    return this.request<{ session_id: string; queue: string[] }>(
      "GET",
      `/api/v1/sessions/${id}/queue`,
    );
  }
  clearQueue(id: string) {
    return this.request<{ status: string; session_id: string; cleared_count: number }>(
      "DELETE",
      `/api/v1/sessions/${id}/queue`,
    );
  }
  cancel(id: string) {
    return this.request<void>("POST", `/api/v1/sessions/${id}/cancel`, {});
  }
  respondPermission(id: string, requestId: string, decision: PermissionDecision) {
    return this.request<void>("POST", `/api/v1/sessions/${id}/permissions/${enc(requestId)}`, {
      decision,
    });
  }
  setPermissionMode(mode: PermissionMode, sessionId?: string) {
    return this.request<void>("POST", "/api/v1/permissions/mode", { mode, session_id: sessionId });
  }
  setSessionModel(id: string, model: string, providerId?: string) {
    return this.request<void>("PUT", `/api/v1/sessions/${id}/model`, {
      model,
      provider_id: providerId,
    });
  }
  providers() {
    return this.request<ProvidersListResponse>("GET", "/api/v1/providers");
  }
  upsertProvider(p: ProviderConfig) {
    return this.request<void>("POST", "/api/v1/providers", p);
  }
  deleteProvider(id: string) {
    return this.request<void>("DELETE", `/api/v1/providers/${enc(id)}`);
  }
  setActiveProvider(providerId: string) {
    return this.request<void>("POST", "/api/v1/providers/active", { provider_id: providerId });
  }
  agentConfig() {
    return this.request<AgentConfig>("GET", "/api/v1/config/agent");
  }
  setAgentConfig(c: SetAgentConfigRequest) {
    return this.request<AgentConfig>("POST", "/api/v1/config/agent", c);
  }
  security() {
    return this.request<SecuritySettings>("GET", "/api/v1/config/security");
  }
  setDefaultPermissionMode(mode: PermissionMode) {
    return this.request<void>("PUT", "/api/v1/config/security", { permission_mode: mode });
  }
  setSandboxMode(mode: "off" | "path_jail" | "bubblewrap" | "auto") {
    return this.request<void>("PUT", "/api/v1/config/security/sandbox", { sandbox_mode: mode });
  }
  execution() {
    return this.request<ExecutionSettings>("GET", "/api/v1/config/execution");
  }
  setExecution(req: SetExecutionRequest) {
    return this.request<ExecutionSettings>("PUT", "/api/v1/config/execution", req);
  }
  /** Asks the lawang daemon what it can run; reaches out over the network, so call on demand. */
  executionStatus() {
    return this.request<ExecutionStatus>("GET", "/api/v1/config/execution/status");
  }
  mcp() {
    return this.request<McpListResponse>("GET", "/api/v1/mcp");
  }
  plugins() {
    return this.request<PluginManifest[]>("GET", "/api/v1/plugins");
  }
  enablePlugin(name: string) {
    return this.request<PluginManifest>("POST", `/api/v1/plugins/${enc(name)}/enable`);
  }
  disablePlugin(name: string) {
    return this.request<PluginManifest>("POST", `/api/v1/plugins/${enc(name)}/disable`);
  }
  skills() {
    return this.request<SkillDefinition[]>("GET", "/api/v1/skills");
  }
  chatgptStatus() {
    return this.request<ChatGptStatus>("GET", "/api/v1/auth/chatgpt");
  }
  chatgptLogin(opts: { newAccount?: boolean; openBrowser?: boolean } = {}) {
    return this.request<ChatGptLoginStarted>("POST", "/api/v1/auth/chatgpt/login", {
      new_account: opts.newAccount ?? false,
      open_browser: opts.openBrowser ?? false,
    });
  }
  chatgptLoginStatus(attemptId: string) {
    return this.request<ChatGptLoginStatus>("GET", `/api/v1/auth/chatgpt/login/${attemptId}`);
  }
  chatgptCancelLogin(attemptId: string) {
    return this.request<void>("DELETE", `/api/v1/auth/chatgpt/login/${attemptId}`);
  }
  chatgptUseAccount(clientId: string) {
    return this.request<void>("POST", "/api/v1/auth/chatgpt/account", { client_id: clientId });
  }
  chatgptLogout() {
    return this.request<void>("DELETE", "/api/v1/auth/chatgpt");
  }
  models(provider?: string) {
    return this.request<ModelsListResponse>(
      "GET",
      `/api/v1/models${provider ? `?provider=${enc(provider)}` : ""}`,
    );
  }
  codebaseMap(id: string, maxTokens = 1000) {
    return this.request<CodebaseMapResponse>("GET", `/api/v1/sessions/${id}/map?max_tokens=${maxTokens}`);
  }
  diffFiles(id: string) {
    return this.request<FileDiffListResponse>("GET", `/api/v1/sessions/${id}/diff/files`);
  }
  fileDiff(id: string, path: string) {
    return this.request<FileDiffResponse>("GET", `/api/v1/sessions/${id}/diff/file?path=${enc(path)}`);
  }
  checkpoints(id: string) {
    return this.request<Checkpoint[]>("GET", `/api/v1/sessions/${id}/checkpoints`);
  }
  undo(id: string, turns?: number) {
    return this.request<UndoResponse>("POST", `/api/v1/sessions/${id}/undo`, { turns });
  }
  worktree(id: string) {
    return this.request<WorktreeStatus>("GET", `/api/v1/sessions/${id}/worktree`);
  }
  commitWorktree(id: string, message?: string) {
    return this.request<{ commit: string | null }>("POST", `/api/v1/sessions/${id}/worktree/commit`, {
      message,
    });
  }
  mergeWorktree(id: string, message?: string) {
    return this.request<MergeOutcome>("POST", `/api/v1/sessions/${id}/worktree/merge`, { message });
  }
  audit(id: string) {
    return this.request<AuditResponse>("GET", `/api/v1/sessions/${id}/audit`);
  }

  // ── Preview lifecycle ────────────────────────────────────────────────────

  startPreview(id: string, req?: PreviewRequest) {
    return this.request<PreviewInfo>("POST", `/api/v1/sessions/${id}/preview`, req ?? {});
  }
  getPreview(id: string) {
    return this.request<PreviewInfo>("GET", `/api/v1/sessions/${id}/preview`);
  }
  stopPreview(id: string) {
    return this.request<PreviewInfo>("DELETE", `/api/v1/sessions/${id}/preview`);
  }
  /** Returns the proxy URL that an iframe `src` can point to for live preview. */
  previewProxyUrl(id: string, path = "/") {
    return `${this.baseUrl}/api/v1/sessions/${id}/preview/proxy${path}`;
  }

  /**
   * Follow an SSE endpoint until `signal` aborts, reconnecting with backoff and resuming from the
   * last seen sequence number so no event is lost or repeated. Uses `fetch` rather than
   * `EventSource` because the latter cannot send an `Authorization` header.
   */
  async follow<T>(path: string, h: StreamHandlers<T>): Promise<void> {
    let last = h.since;
    let delay = 500;
    while (!h.signal.aborted) {
      try {
        const sep = path.includes("?") ? "&" : "?";
        const url = `${this.baseUrl}${path}${last === undefined ? "" : `${sep}since=${last}`}`;
        const res = await fetch(url, {
          headers: this.headers({ Accept: "text/event-stream" }),
          signal: h.signal,
        });
        if (!res.ok || !res.body) throw new ApiError(res.status, await res.text());
        h.onStatus?.("connected");
        delay = 500;
        const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
        const parser = new SseParser();
        for (;;) {
          const { value, done } = await reader.read();
          if (done) break;
          for (const msg of parser.push(value)) {
            const seq = msg.id === undefined ? undefined : Number(msg.id);
            if (seq !== undefined && last !== undefined && seq <= last) continue;
            if (seq !== undefined) last = seq;
            h.onEvent(JSON.parse(msg.data) as T, seq);
          }
        }
      } catch (error) {
        if (h.signal.aborted) return;
        if (error instanceof ApiError && (error.status === 401 || error.status === 404)) throw error;
      }
      if (h.signal.aborted) return;
      h.onStatus?.("reconnecting");
      await new Promise((r) => setTimeout(r, delay));
      delay = Math.min(delay * 2, 5000);
    }
  }
}
