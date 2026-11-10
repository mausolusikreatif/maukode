import type { SessionListItem } from "../api/types";
import { baseName } from "./format";
import type { ThreadState } from "./thread";

export type SessionStatusFilter = "all" | "needs_you" | "running" | "completed" | "archived";

export const PINNED_SESSIONS_STORAGE_KEY = "maukode.pinnedSessions";
export const ARCHIVED_SESSIONS_STORAGE_KEY = "maukode.archivedSessions";

/** Load pinned session IDs from localStorage. */
export function loadPinnedSessions(): string[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const val: unknown = JSON.parse(localStorage.getItem(PINNED_SESSIONS_STORAGE_KEY) ?? "[]");
    return Array.isArray(val) ? val.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** Save pinned session IDs to localStorage. */
export function savePinnedSessions(ids: Iterable<string>): void {
  if (typeof localStorage === "undefined") return;
  try {
    const arr = Array.from(new Set(ids));
    localStorage.setItem(PINNED_SESSIONS_STORAGE_KEY, JSON.stringify(arr));
  } catch {
    // Storage may be unavailable or disabled
  }
}

/** Load archived session IDs from localStorage. */
export function loadArchivedSessions(): string[] {
  if (typeof localStorage === "undefined") return [];
  try {
    const val: unknown = JSON.parse(localStorage.getItem(ARCHIVED_SESSIONS_STORAGE_KEY) ?? "[]");
    return Array.isArray(val) ? val.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

/** Save archived session IDs to localStorage. */
export function saveArchivedSessions(ids: Iterable<string>): void {
  if (typeof localStorage === "undefined") return;
  try {
    const arr = Array.from(new Set(ids));
    localStorage.setItem(ARCHIVED_SESSIONS_STORAGE_KEY, JSON.stringify(arr));
  } catch {
    // Storage may be unavailable or disabled
  }
}

/** The group every chat-only session belongs to; it is shown under this name. */
export const CHAT_GROUP = "Chat";

/** Returns the project directory or directory path for grouping. Chats have no project. */
export function projectOfSession(s: SessionListItem): string {
  if (s.chat) return CHAT_GROUP;
  return s.project_dir ?? s.directory;
}

/** Check if a session matches a full-text search query across prompt/goal, repo name, directory, branch, or messages. */
export function matchesSearch(
  session: SessionListItem,
  query: string,
  thread?: ThreadState,
): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const tokens = q.split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return true;

  const projectDir = projectOfSession(session).toLowerCase();
  const repo = baseName(projectOfSession(session)).toLowerCase();
  const dir = session.directory.toLowerCase();
  const title = (session.title || "").toLowerCase();
  const branch = (session.worktree_branch || "").toLowerCase();
  const messages =
    thread?.items
      ?.filter((item) => item.kind === "user" || item.kind === "assistant")
      ?.map((item) => item.text.toLowerCase()) ?? [];

  return tokens.every((token) => {
    if (title.includes(token)) return true;
    if (repo.includes(token)) return true;
    if (projectDir.includes(token)) return true;
    if (dir.includes(token)) return true;
    if (branch.includes(token)) return true;
    if (messages.some((msg) => msg.includes(token))) return true;
    return false;
  });
}

/** Check if a session matches the selected status filter. */
export function matchesStatus(
  session: SessionListItem,
  filter: SessionStatusFilter,
  archivedIds: ReadonlySet<string>,
): boolean {
  const isArchived = archivedIds.has(session.id);
  if (filter === "archived") {
    return isArchived;
  }
  // All other filters only consider non-archived sessions
  if (isArchived) {
    return false;
  }
  switch (filter) {
    case "all":
      return true;
    case "needs_you":
      return session.pending_permission_count > 0;
    case "running":
      return session.running;
    case "completed":
      return !session.running && session.pending_permission_count === 0;
  }
}

/** Counts of sessions in each status category. */
export function getStatusCounts(
  sessions: SessionListItem[],
  archivedIds: ReadonlySet<string>,
): Record<SessionStatusFilter, number> {
  let all = 0;
  let needsYou = 0;
  let running = 0;
  let completed = 0;
  let archived = 0;

  for (const s of sessions) {
    if (archivedIds.has(s.id)) {
      archived++;
    } else {
      all++;
      if (s.pending_permission_count > 0) needsYou++;
      if (s.running) running++;
      if (!s.running && s.pending_permission_count === 0) completed++;
    }
  }

  return {
    all,
    needs_you: needsYou,
    running,
    completed,
    archived,
  };
}

/** Sort sessions so pinned sessions appear first, then sorted newest first. */
export function sortSessions(
  sessions: SessionListItem[],
  pinnedIds: ReadonlySet<string>,
): SessionListItem[] {
  return [...sessions].sort((a, b) => {
    const aPinned = pinnedIds.has(a.id);
    const bPinned = pinnedIds.has(b.id);
    if (aPinned !== bPinned) return aPinned ? -1 : 1;
    return b.updated_at.localeCompare(a.updated_at);
  });
}

/** Filter sessions by status, search query, and threads. */
export function filterSessions(
  sessions: SessionListItem[],
  options: {
    query?: string;
    status?: SessionStatusFilter;
    archivedIds?: ReadonlySet<string>;
    threads?: Record<string, ThreadState>;
  },
): SessionListItem[] {
  const status = options.status ?? "all";
  const query = options.query ?? "";
  const archived = options.archivedIds ?? new Set<string>();
  const threads = options.threads ?? {};

  return sessions.filter((s) => {
    if (!matchesStatus(s, status, archived)) return false;
    if (!matchesSearch(s, query, threads[s.id])) return false;
    return true;
  });
}

/** Group filtered sessions by project directory, with pinned items first. */
export function groupSessionsByProject(
  sessions: SessionListItem[],
  pinnedIds: ReadonlySet<string>,
): [string, SessionListItem[]][] {
  const map = new Map<string, SessionListItem[]>();
  for (const s of sessions) {
    const key = projectOfSession(s);
    const list = map.get(key);
    if (list) {
      list.push(s);
    } else {
      map.set(key, [s]);
    }
  }

  const entries: [string, SessionListItem[]][] = [...map.entries()].map(([project, items]) => [
    project,
    sortSessions(items, pinnedIds),
  ]);

  return entries.sort(([, itemsA], [, itemsB]) => {
    const aHasPinned = itemsA.some((s) => pinnedIds.has(s.id));
    const bHasPinned = itemsB.some((s) => pinnedIds.has(s.id));
    if (aHasPinned !== bHasPinned) return aHasPinned ? -1 : 1;
    const aLatest = itemsA[0]?.updated_at ?? "";
    const bLatest = itemsB[0]?.updated_at ?? "";
    return bLatest.localeCompare(aLatest);
  });
}
