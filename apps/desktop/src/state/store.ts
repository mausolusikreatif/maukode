import { create } from "zustand";
import { ApiClient, ApiError } from "../api/client";
import type {
  CreateSessionRequest,
  FileDiffEntry,
  FileDiffResponse,
  GlobalEvent,
  LLMEvent,
  ExecutionSettings,
  PermissionDecision,
  PermissionRequest,
  PreviewInfo,
  PromptImage,
  ProvidersListResponse,
  SessionDetailResponse,
  SessionListItem,
  StopReason,
  WorktreeStatus,
} from "../api/types";
import { focusWindow, forgetBootstrap, saveBootstrap } from "../platform";
import {
  disableNotifications,
  enableNotifications,
  loadNotificationsPref,
  noticeFor,
  showNotice,
} from "./notify";
import {
  loadArchivedSessions,
  loadPinnedSessions,
  saveArchivedSessions,
  savePinnedSessions,
  type SessionStatusFilter,
} from "./sessionFilters";
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

/** Result of the last review action, shown under the buttons that triggered it. */
export interface ActionResult {
  level: "ok" | "warn" | "error";
  text: string;
  /** Files listed for a merge conflict. */
  files?: string[];
}

/** State for the live dev-server preview panel. */
export interface PreviewState {
  open: boolean;
  loading: boolean;
  info: PreviewInfo | null;
  error: string | null;
}

const initialPreview: PreviewState = {
  open: false,
  loading: false,
  info: null,
  error: null,
};

/** What the agent changed in the open session, and what can be done about it. */
export interface ReviewState {
  open: boolean;
  loading: boolean;
  error: string | null;
  files: FileDiffEntry[];
  selected: string | null;
  diff: FileDiffResponse | null;
  checkpoints: number;
  worktree: WorktreeStatus | null;
  busy: boolean;
  result: ActionResult | null;
}

export const emptyReview = (open = false): ReviewState => ({
  open, loading: false, error: null, files: [], selected: null, diff: null,
  checkpoints: 0, worktree: null, busy: false, result: null,
});

/** How a session's most recent turn ended (kept for this app run; the server does not store it). */
export interface LastStop {
  reason: StopReason;
  message: string | null;
}

interface AppState {
  client: ApiClient | null;
  connection: Connection;
  connectionError: string | null;
  sessions: SessionListItem[];
  /** Permission requests waiting on the user, across every session. */
  inbox: PermissionRequest[];
  providers: ProvidersListResponse | null;
  /** Where commands run (host or a lawang cell); labels sessions. `null` until loaded. */
  execution: ExecutionSettings | null;
  activeId: string | null;
  meta: SessionMeta | null;
  threads: Record<string, ThreadState>;
  loadingOlder: boolean;
  review: ReviewState;
  /** Main area when no session is open: the overview, or the settings screen. */
  view: "home" | "settings";
  lastStop: Record<string, LastStop>;
  notificationsOn: boolean;
  sessionSearchQuery: string;
  sessionStatusFilter: SessionStatusFilter;
  pinnedSessionIds: string[];
  archivedSessionIds: string[];

  setSessionSearchQuery: (query: string) => void;
  setSessionStatusFilter: (filter: SessionStatusFilter) => void;
  togglePinSession: (id: string) => void;
  toggleArchiveSession: (id: string) => Promise<void>;
  /** Open a file of the active session in the daemon machine's editor. */
  openInEditor: (path: string, line?: number) => Promise<void>;

  refreshProviders: () => Promise<void>;
  goHome: () => void;
  openSettings: () => void;
  setNotifications: (on: boolean) => Promise<boolean>;
  connect: (url: string, token?: string) => Promise<void>;
  disconnect: () => void;
  refreshSessions: () => Promise<void>;
  refreshExecution: () => Promise<void>;
  openSession: (id: string) => Promise<void>;
  closeSession: () => void;
  createSession: (req: CreateSessionRequest) => Promise<void>;
  deleteSession: (id: string, opts?: { cleanupWorktree?: boolean; deleteBranch?: boolean }) => Promise<void>;
  renameSession: (id: string, title: string) => Promise<void>;
  setSessionModel: (id: string, model: string, providerId?: string) => Promise<void>;
  send: (text: string, images?: PromptImage[]) => Promise<void>;
  steer: (text: string) => Promise<void>;
  queue: (text: string) => Promise<void>;
  clearQueue: () => Promise<void>;
  stop: () => Promise<void>;
  respond: (requestId: string, decision: PermissionDecision) => Promise<void>;
  /** Answer a permission request in any session, not only the open one. */
  respondTo: (sessionId: string, requestId: string, decision: PermissionDecision) => Promise<void>;
  loadOlder: () => Promise<void>;

  toggleReview: () => Promise<void>;
  refreshReview: () => Promise<void>;
  selectFile: (path: string) => Promise<void>;
  undoLast: () => Promise<void>;
  commitWorktree: (message: string) => Promise<void>;
  mergeWorktree: (message: string) => Promise<void>;

  // Preview
  preview: PreviewState;
  startPreview: (id: string, port?: number) => Promise<void>;
  refreshPreview: (id: string) => Promise<void>;
  stopPreview: (id: string) => Promise<void>;
  togglePreview: () => void;

  sidebarOpen: boolean;
  newSessionOpen: boolean;
  newSessionInitialDir: string | null;
  shortcutsOpen: boolean;
  commandPaletteOpen: boolean;

  toggleSidebar: () => void;
  openNewSession: (dir?: string | null) => void;
  closeNewSession: () => void;
  toggleShortcuts: () => void;
  closeShortcuts: () => void;
  toggleCommandPalette: () => void;
  closeCommandPalette: () => void;
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

  /** Load the requests already waiting (e.g. after connecting or reconnecting). */
  const hydrateInbox = async () => {
    const { client, sessions } = get();
    if (!client) return;
    const waiting = sessions.filter((x) => x.pending_permission_count > 0);
    const lists = await Promise.all(
      waiting.map((x) =>
        client
          .getSession(x.id, 1)
          .then((d) => d.pending_permissions)
          .catch(() => [] as PermissionRequest[]),
      ),
    );
    if (get().client === client) set({ inbox: lists.flat() });
  };

  const onGlobal = (e: GlobalEvent) => {
    const st = get();
    if (st.notificationsOn) {
      const notice = noticeFor(e, {
        activeId: st.activeId,
        windowHidden: document.hidden || !document.hasFocus(),
        sessions: st.sessions,
      });
      if (notice) {
        const id = "session_id" in e ? e.session_id : null;
        showNotice(notice, () => {
          void focusWindow();
          if (id) void get().openSession(id);
        });
      }
    }
    switch (e.type) {
      case "session_deleted":
        set((s) => ({ inbox: s.inbox.filter((r) => r.session_id !== e.session_id) }));
        refreshSoon();
        break;
      case "session_created":
      case "session_updated":
        refreshSoon();
        break;
      case "turn_started":
        patchSession(e.session_id, (x) => ({ ...x, running: true }));
        refreshSoon();
        set((s) => {
          const { [e.session_id]: _gone, ...rest } = s.lastStop;
          return { lastStop: rest };
        });
        break;
      case "turn_finished":
        set((s) => ({ inbox: s.inbox.filter((r) => r.session_id !== e.session_id) }));
        patchSession(e.session_id, (x) => ({ ...x, running: false, pending_permission_count: 0 }));
        set((s) => ({ lastStop: { ...s.lastStop, [e.session_id]: { reason: e.reason, message: e.message } } }));
        refreshSoon();
        break;
      case "permission_requested":
        set((s) => ({
          inbox: [...s.inbox.filter((r) => r.request_id !== e.request.request_id), e.request],
        }));
        patchSession(e.session_id, (x) => ({ ...x, pending_permission_count: x.pending_permission_count + 1 }));
        break;
      case "permission_resolved":
        set((s) => ({ inbox: s.inbox.filter((r) => r.request_id !== e.request_id) }));
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
      patchThread(id, (t) => ({
        ...t,
        items: itemsFromMessages(d.messages),
        queue: d.queued_prompts ?? t.queue,
      }));
      set({ meta: metaFrom(d) });
    } catch {
      /* the live stream already showed the turn; a failed resync is not worth interrupting */
    }
  };

  const onSessionEvent = (id: string) => (event: LLMEvent, seq: number | undefined) => {
    patchThread(id, (t) => applyEvent(t, event, seq));
    if (event.type === "turn_stopped") {
      void resync(id);
      if (get().review.open) void get().refreshReview(); // the agent just finished changing files
    }
  };

  const patchReview = (p: Partial<ReviewState>) => set((s) => ({ review: { ...s.review, ...p } }));

  /** Run a review action against the open session; the result (or error) is shown to the user. */
  const reviewAction = async (run: (client: ApiClient, id: string) => Promise<ActionResult>) => {
    const { client, activeId } = get();
    if (!client || !activeId) return;
    patchReview({ busy: true, result: null });
    try {
      const result = await run(client, activeId);
      if (get().activeId !== activeId) return;
      patchReview({ result });
    } catch (e) {
      if (get().activeId !== activeId) return;
      patchReview({ result: { level: "error", text: errorText(e) } });
    } finally {
      patchReview({ busy: false });
      await get().refreshReview();
      void get().refreshSessions();
    }
  };

  return {
    client: null,
    execution: null,
    connection: "disconnected",
    connectionError: null,
    sessions: [],
    inbox: [],
    providers: null,
    activeId: null,
    meta: null,
    threads: {},
    loadingOlder: false,
    review: emptyReview(),
    preview: initialPreview,
    view: "home",
    lastStop: {},
    notificationsOn: loadNotificationsPref(),
    sidebarOpen: true,
    newSessionOpen: false,
    newSessionInitialDir: null,
    shortcutsOpen: false,
    commandPaletteOpen: false,

    sessionSearchQuery: "",
    sessionStatusFilter: "all",
    pinnedSessionIds: loadPinnedSessions(),
    archivedSessionIds: [],

    setSessionSearchQuery: (query) => set({ sessionSearchQuery: query }),
    setSessionStatusFilter: (filter) => set({ sessionStatusFilter: filter }),
    togglePinSession: (id) => {
      const current = get().pinnedSessionIds;
      const next = current.includes(id) ? current.filter((x) => x !== id) : [...current, id];
      savePinnedSessions(next);
      set({ pinnedSessionIds: next });
    },
    toggleArchiveSession: async (id) => {
      const client = get().client;
      if (!client) return;
      const archived = !get().archivedSessionIds.includes(id);
      // Optimistic: the list updates at once and the refresh confirms it.
      set((s) => ({
        archivedSessionIds: archived ? [...s.archivedSessionIds, id] : s.archivedSessionIds.filter((x) => x !== id),
      }));
      try {
        await client.archiveSession(id, archived);
      } finally {
        await get().refreshSessions();
      }
    },

    openInEditor: async (path, line) => {
      const { client, activeId } = get();
      if (!client || !activeId) return;
      try {
        await client.openInEditor(activeId, path, line ?? null);
      } catch (e) {
        patchThread(activeId, (t) => addNotice(t, "error", `Gagal membuka editor: ${errorText(e)}`));
      }
    },

    toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
    openNewSession: (dir = null) => set({ newSessionOpen: true, newSessionInitialDir: dir }),
    closeNewSession: () => set({ newSessionOpen: false, newSessionInitialDir: null }),
    toggleShortcuts: () => set((s) => ({ shortcutsOpen: !s.shortcutsOpen })),
    closeShortcuts: () => set({ shortcutsOpen: false }),
    toggleCommandPalette: () => set((s) => ({ commandPaletteOpen: !s.commandPaletteOpen })),
    closeCommandPalette: () => set({ commandPaletteOpen: false }),

    // ── Preview ──────────────────────────────────────────────────────────
    startPreview: async (id, port) => {
      const { client } = get();
      if (!client) return;
      set((s) => ({ preview: { ...s.preview, loading: true, error: null } }));
      try {
        const info = await client.startPreview(id, port ? { port, sandbox_mode: null } : undefined);
        set({
          preview: {
            open: true,
            loading: false,
            info,
            error: null,
          },
        });
      } catch (e) {
        set((s) => ({ preview: { ...s.preview, loading: false, error: errorText(e) } }));
      }
    },
    refreshPreview: async (id) => {
      const { client } = get();
      if (!client) return;
      try {
        const info = await client.getPreview(id);
        set((s) => ({
          preview: {
            ...s.preview,
            info,
            error: null,
          },
        }));
      } catch (e) {
        set((s) => ({ preview: { ...s.preview, error: errorText(e) } }));
      }
    },
    stopPreview: async (id) => {
      const { client } = get();
      if (!client) return;
      try {
        await client.stopPreview(id);
        set((s) => ({ preview: { ...s.preview, info: null, open: false } }));
      } catch (e) {
        set((s) => ({ preview: { ...s.preview, error: errorText(e) } }));
      }
    },
    togglePreview: () => set((s) => ({ preview: { ...s.preview, open: !s.preview.open } })),

    refreshProviders: async () => {
      const client = get().client;
      if (!client) return;
      set({ providers: await client.providers() });
    },
    goHome: () => {
      get().closeSession();
      set({ view: "home" });
    },
    openSettings: () => {
      get().closeSession();
      set({ view: "settings" });
    },
    setNotifications: async (on) => {
      const granted = on ? await enableNotifications() : false;
      if (!on) disableNotifications();
      set({ notificationsOn: granted });
      return granted;
    },

    connect: async (url, token) => {
      get().disconnect();
      const client = new ApiClient(url.replace(/\/+$/, ""), token || undefined);
      set({ connection: "connecting", connectionError: null });
      try {
        if (!(await client.health())) throw new Error(`No MauKode server answered at ${url}`);
        const { sessions, eventSeq } = await client.listSessions(true);
        const providers = await client.providers();
        set({ client, sessions, providers, connection: "online", inbox: [] });
        saveBootstrap({ url: client.baseUrl, token });
        void hydrateInbox();
        // Not fatal: an older server has no such setting, and the label simply stays hidden.
        void get().refreshExecution();

        const ctrl = new AbortController();
        globalAbort = ctrl;
        void client
          .follow<GlobalEvent>("/api/v1/events", {
            since: eventSeq,
            signal: ctrl.signal,
            onEvent: (e) => onGlobal(e),
            onStatus: (st) => {
              set({ connection: st === "connected" ? "online" : "reconnecting" });
              if (st === "connected") void get().refreshSessions().then(hydrateInbox);
            },
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
      set({ client: null, connection: "disconnected", sessions: [], inbox: [], activeId: null, meta: null, threads: {}, lastStop: {}, view: "home" });
    },

    refreshExecution: async () => {
      const client = get().client;
      if (!client) return;
      try {
        set({ execution: await client.execution() });
      } catch {
        set({ execution: null });
      }
    },

    refreshSessions: async () => {
      const client = get().client;
      if (!client) return;
      try {
        const { sessions } = await client.listSessions(true);
        set({ sessions, archivedSessionIds: sessions.filter((s) => s.archived).map((s) => s.id) });
        // Archives used to live in this browser only: move them to the daemon once.
        const local = loadArchivedSessions().filter((id) => sessions.some((s) => s.id === id && !s.archived));
        if (local.length > 0) {
          saveArchivedSessions([]);
          await Promise.allSettled(local.map((id) => client.archiveSession(id, true)));
          await get().refreshSessions();
        }
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
      set((s) => ({ activeId: id, meta: null, view: "home", review: emptyReview(s.review.open), preview: { ...initialPreview, open: s.preview.open } }));
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
              queue: d.queued_prompts ?? [],
              sessionUsage: d.usage ?? null,
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
        if (get().review.open) void get().refreshReview();
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

    deleteSession: async (id, opts) => {
      const client = get().client;
      if (!client) return;
      await client.deleteSession(id, opts);
      if (get().activeId === id) get().closeSession();
      const pinned = get().pinnedSessionIds.filter((x) => x !== id);
      const archived = get().archivedSessionIds.filter((x) => x !== id);
      savePinnedSessions(pinned);
      set({ pinnedSessionIds: pinned, archivedSessionIds: archived });
      await get().refreshSessions();
    },

    renameSession: async (id, title) => {
      const client = get().client;
      if (!client) return;
      const clean = title.trim() || null;
      await client.renameSession(id, clean);
      if (get().activeId === id) {
        set((s) => ({ meta: s.meta ? { ...s.meta, title: clean } : null }));
      }
      patchSession(id, (s) => ({ ...s, title: clean ?? s.title }));
    },

    setSessionModel: async (id, model, providerId) => {
      const client = get().client;
      if (!client) return;
      await client.setSessionModel(id, model, providerId);
      if (get().activeId === id) {
        set((s) => ({
          meta: s.meta
            ? { ...s.meta, model, provider: providerId ?? s.meta.provider }
            : null,
        }));
      }
      patchSession(id, (s) => ({
        ...s,
        model,
        provider: providerId ?? s.provider,
      }));
    },

    send: async (text, images = []) => {
      const { client, activeId } = get();
      if (!client || !activeId || (!text.trim() && images.length === 0)) return;
      patchThread(activeId, (t) => startTurn(t, text, images));
      try {
        await client.prompt(activeId, text, images);
      } catch (e) {
        patchThread(activeId, (t) => ({ ...addNotice(t, "error", errorText(e)), running: false }));
      }
    },

    steer: async (text) => {
      const { client, activeId } = get();
      if (!client || !activeId || !text.trim()) return;
      patchThread(activeId, (t) => ({
        ...t,
        items: [...t.items, { kind: "user", id: `s${t.nextId}`, text: `[Steer] ${text}` }],
        nextId: t.nextId + 1,
      }));
      try {
        await client.steerPrompt(activeId, text);
      } catch (e) {
        patchThread(activeId, (t) => addNotice(t, "error", errorText(e)));
      }
    },

    queue: async (text) => {
      const { client, activeId } = get();
      if (!client || !activeId || !text.trim()) return;
      patchThread(activeId, (t) => ({
        ...t,
        queue: [...t.queue, text],
      }));
      try {
        await client.queuePrompt(activeId, text);
      } catch (e) {
        patchThread(activeId, (t) => addNotice(t, "error", errorText(e)));
      }
    },

    clearQueue: async () => {
      const { client, activeId } = get();
      if (!client || !activeId) return;
      patchThread(activeId, (t) => ({ ...t, queue: [] }));
      try {
        await client.clearQueue(activeId);
      } catch (e) {
        patchThread(activeId, (t) => addNotice(t, "error", errorText(e)));
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

    respondTo: async (sessionId, requestId, decision) => {
      const { client } = get();
      if (!client) return;
      set((s) => ({ inbox: s.inbox.filter((r) => r.request_id !== requestId) }));
      patchThread(sessionId, (t) => resolvePermission(t, requestId));
      try {
        await client.respondPermission(sessionId, requestId, decision);
      } catch {
        // The request may already have been answered elsewhere; resync what is really pending.
        await get().refreshSessions();
        await hydrateInbox();
      }
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

    toggleReview: async () => {
      const open = !get().review.open;
      patchReview({ open });
      if (open) await get().refreshReview();
    },

    refreshReview: async () => {
      const { client, activeId, meta } = get();
      if (!client || !activeId) return;
      patchReview({ loading: true, error: null });
      try {
        const [list, checkpoints, worktree] = await Promise.all([
          client.diffFiles(activeId),
          client.checkpoints(activeId),
          meta?.branch ? client.worktree(activeId).catch(() => null) : Promise.resolve(null),
        ]);
        if (get().activeId !== activeId) return;
        const files = list.files;
        const keep = get().review.selected;
        const selected = files.some((f) => f.path === keep) ? keep : (files[0]?.path ?? null);
        patchReview({ files, checkpoints: checkpoints.length, worktree, selected, loading: false });
        if (selected) await get().selectFile(selected);
        else patchReview({ diff: null });
      } catch (e) {
        if (get().activeId === activeId) patchReview({ loading: false, error: errorText(e) });
      }
    },

    selectFile: async (path) => {
      const { client, activeId } = get();
      if (!client || !activeId) return;
      patchReview({ selected: path });
      try {
        const diff = await client.fileDiff(activeId, path);
        if (get().activeId === activeId && get().review.selected === path) patchReview({ diff, error: null });
      } catch (e) {
        if (get().activeId === activeId) patchReview({ diff: null, error: errorText(e) });
      }
    },

    undoLast: () =>
      reviewAction(async (client, id) => {
        const r = await client.undo(id, 1);
        return { level: "ok", text: r.message };
      }),

    commitWorktree: (message) =>
      reviewAction(async (client, id) => {
        const r = await client.commitWorktree(id, message.trim() || undefined);
        return r.commit
          ? { level: "ok", text: `Committed ${r.commit.slice(0, 8)}.` }
          : { level: "warn", text: "Nothing to commit." };
      }),

    mergeWorktree: (message) =>
      reviewAction(async (client, id) => {
        const r = await client.mergeWorktree(id, message.trim() || undefined);
        switch (r.result) {
          case "merged":
            return { level: "ok", text: `Merged into the project (${r.commit.slice(0, 8)}).` };
          case "nothing_to_merge":
            return { level: "warn", text: "Nothing to merge: the branch has no new commits." };
          case "conflict":
            return {
              level: "error",
              text: "Merge conflict. Nothing was changed in your project; resolve it by hand or ask the agent to rebase.",
              files: r.files,
            };
        }
      }),
  };
});
