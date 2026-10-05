import { create } from "zustand";
import { ApiClient, ApiError } from "../api/client";
import type {
  CreateSessionRequest,
  GlobalEvent,
  LLMEvent,
  PermissionDecision,
  ProvidersListResponse,
  SessionDetailResponse,
  SessionListItem,
} from "../api/types";
import { forgetBootstrap, saveBootstrap } from "../platform";
import {
  addNotice,
  applyEvent,
  emptyThread,
  itemsFromMessages,
  resolvePermission,
  startTurn,
  threadFromSnapshot,
  type ThreadState,
} from "./thread";

export type Connection = "disconnected" | "connecting" | "online" | "reconnecting";

/** Header info for the open session. */
export interface SessionMeta {
  title: string | null;
  directory: string;
  provider: string | null;
  model: string | null;
  branch: string | null;
  projectDir: string | null;
  /** Index in the full history of the first loaded message (older ones can be fetched). */
  loadedStart: number;
}

interface AppState {
  client: ApiClient | null;
  connection: Connection;
  connectionError: string | null;
  sessions: SessionListItem[];
  providers: ProvidersListResponse | null;
  activeId: string | null;
  meta: SessionMeta | null;
  threads: Record<string, ThreadState>;
  loadingOlder: boolean;

  connect: (url: string, token?: string) => Promise<void>;
  disconnect: () => void;
  refreshSessions: () => Promise<void>;
  openSession: (id: string) => Promise<void>;
  closeSession: () => void;
  createSession: (req: CreateSessionRequest) => Promise<void>;
  deleteSession: (id: string) => Promise<void>;
  send: (text: string) => Promise<void>;
  stop: () => Promise<void>;
  respond: (requestId: string, decision: PermissionDecision) => Promise<void>;
  loadOlder: () => Promise<void>;
}

// Stream controllers live outside the store: they are resources, not UI state.
let globalAbort: AbortController | null = null;
let sessionAbort: AbortController | null = null;

const PAGE = 100;

function metaFrom(d: SessionDetailResponse): SessionMeta {
  return {
    title: d.title,
    directory: d.directory,
    provider: d.provider,
    model: d.model,
    branch: d.worktree?.branch ?? null,
    projectDir: d.worktree?.project_dir ?? null,
    loadedStart: d.messages_start,
  };
}

function errorText(e: unknown): string {
  return e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e);
}

export const useApp = create<AppState>((set, get) => {
  const patchThread = (id: string, fn: (t: ThreadState) => ThreadState) =>
    set((s) => ({ threads: { ...s.threads, [id]: fn(s.threads[id] ?? emptyThread()) } }));

  const patchSession = (id: string, fn: (s: SessionListItem) => SessionListItem) =>
    set((s) => ({ sessions: s.sessions.map((x) => (x.id === id ? fn(x) : x)) }));

  let refreshTimer: ReturnType<typeof setTimeout> | undefined;
  const refreshSoon = () => {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(() => void get().refreshSessions(), 150);
  };

  const onGlobal = (e: GlobalEvent) => {
    switch (e.type) {
      case "session_created":
      case "session_deleted":
      case "session_updated":
        refreshSoon();
        break;
      case "turn_started":
        patchSession(e.session_id, (x) => ({ ...x, running: true }));
        break;
      case "turn_finished":
        patchSession(e.session_id, (x) => ({ ...x, running: false, pending_permission_count: 0 }));
        refreshSoon();
        break;
      case "permission_requested":
        patchSession(e.session_id, (x) => ({ ...x, pending_permission_count: x.pending_permission_count + 1 }));
        break;
      case "permission_resolved":
        patchSession(e.session_id, (x) => ({
          ...x,
          pending_permission_count: Math.max(0, x.pending_permission_count - 1),
        }));
        break;
    }
  };

  /** Rebuild the open thread from stored history; heals any gap left by a mid-step snapshot. */
  const resync = async (id: string) => {
    const client = get().client;
    if (!client) return;
    try {
      const d = await client.getSession(id, 500);
      if (get().activeId !== id) return;
      patchThread(id, (t) => ({ ...t, items: itemsFromMessages(d.messages) }));
      set({ meta: metaFrom(d) });
    } catch {
      /* the live stream already showed the turn; a failed resync is not worth interrupting */
    }
  };

  const onSessionEvent = (id: string) => (event: LLMEvent, seq: number | undefined) => {
    patchThread(id, (t) => applyEvent(t, event, seq));
    if (event.type === "turn_stopped") void resync(id);
  };

  return {
    client: null,
    connection: "disconnected",
    connectionError: null,
    sessions: [],
    providers: null,
    activeId: null,
    meta: null,
    threads: {},
    loadingOlder: false,

    connect: async (url, token) => {
      get().disconnect();
      const client = new ApiClient(url.replace(/\/+$/, ""), token || undefined);
      set({ connection: "connecting", connectionError: null });
      try {
        if (!(await client.health())) throw new Error(`No MauKode server answered at ${url}`);
        const { sessions, eventSeq } = await client.listSessions(true);
        const providers = await client.providers();
        set({ client, sessions, providers, connection: "online" });
        saveBootstrap({ url: client.baseUrl, token });

        const ctrl = new AbortController();
        globalAbort = ctrl;
        void client
          .follow<GlobalEvent>("/api/v1/events", {
            since: eventSeq,
            signal: ctrl.signal,
            onEvent: (e) => onGlobal(e),
            onStatus: (st) => set({ connection: st === "connected" ? "online" : "reconnecting" }),
          })
          .catch((e) => {
            // 401/404 are not retryable: the token or server is wrong.
            forgetBootstrap();
            set({ connection: "disconnected", connectionError: errorText(e), client: null });
          });
      } catch (e) {
        set({ connection: "disconnected", connectionError: errorText(e), client: null });
      }
    },

    disconnect: () => {
      globalAbort?.abort();
      sessionAbort?.abort();
      globalAbort = sessionAbort = null;
      set({ client: null, connection: "disconnected", sessions: [], activeId: null, meta: null, threads: {} });
    },

    refreshSessions: async () => {
      const client = get().client;
      if (!client) return;
      try {
        const { sessions } = await client.listSessions(true);
        set({ sessions });
      } catch {
        /* transient; the next event triggers another refresh */
      }
    },

    openSession: async (id) => {
      const client = get().client;
      if (!client) return;
      sessionAbort?.abort();
      const ctrl = new AbortController();
      sessionAbort = ctrl;
      set({ activeId: id, meta: null });
      try {
        const d = await client.getSession(id, PAGE);
        if (ctrl.signal.aborted) return;
        set((s) => ({
          meta: metaFrom(d),
          threads: {
            ...s.threads,
            [id]: threadFromSnapshot(d.messages, {
              running: d.running,
              pending: d.pending_permissions,
              lastSeq: d.last_event_seq,
            }),
          },
        }));
        void client
          .follow<LLMEvent>(`/api/v1/sessions/${id}/events`, {
            since: d.last_event_seq,
            signal: ctrl.signal,
            onEvent: onSessionEvent(id),
          })
          .catch(() => undefined);
      } catch (e) {
        if (ctrl.signal.aborted) return;
        patchThread(id, (t) => addNotice(t, "error", errorText(e)));
      }
    },

    closeSession: () => {
      sessionAbort?.abort();
      sessionAbort = null;
      set({ activeId: null, meta: null });
    },

    createSession: async (req) => {
      const client = get().client;
      if (!client) return;
      const created = await client.createSession(req);
      await get().refreshSessions();
      await get().openSession(created.id);
    },

    deleteSession: async (id) => {
      const client = get().client;
      if (!client) return;
      await client.deleteSession(id);
      if (get().activeId === id) get().closeSession();
      await get().refreshSessions();
    },

    send: async (text) => {
      const { client, activeId } = get();
      if (!client || !activeId || !text.trim()) return;
      patchThread(activeId, (t) => startTurn(t, text));
      try {
        await client.prompt(activeId, text);
      } catch (e) {
        patchThread(activeId, (t) => ({ ...addNotice(t, "error", errorText(e)), running: false }));
      }
    },

    stop: async () => {
      const { client, activeId } = get();
      if (client && activeId) await client.cancel(activeId).catch(() => undefined);
    },

    respond: async (requestId, decision) => {
      const { client, activeId } = get();
      if (!client || !activeId) return;
      patchThread(activeId, (t) => resolvePermission(t, requestId));
      await client.respondPermission(activeId, requestId, decision).catch(() => undefined);
    },

    loadOlder: async () => {
      const { client, activeId, meta, loadingOlder } = get();
      if (!client || !activeId || !meta || meta.loadedStart <= 0 || loadingOlder) return;
      set({ loadingOlder: true });
      try {
        const page = await client.messages(activeId, meta.loadedStart, PAGE);
        const older = itemsFromMessages(page.messages, `o${page.start}-`);
        patchThread(activeId, (t) => ({ ...t, items: [...older, ...t.items] }));
        set({ meta: { ...meta, loadedStart: page.start } });
      } finally {
        set({ loadingOlder: false });
      }
    },
  };
});
