import { beforeEach, describe, expect, it } from "vitest";
import type { SessionListItem } from "../api/types";
import {
  filterSessions,
  getStatusCounts,
  groupSessionsByProject,
  loadArchivedSessions,
  loadPinnedSessions,
  matchesSearch,
  matchesStatus,
  projectOfSession,
  saveArchivedSessions,
  savePinnedSessions,
  sortSessions,
} from "./sessionFilters";
import { emptyThread, type ThreadState } from "./thread";

const mockSession = (overrides: Partial<SessionListItem> = {}): SessionListItem => ({
  id: "session-1",
  directory: "/home/user/code/maukode",
  project_dir: null,
  title: "Implement auth token refresh",
  message_count: 5,
  provider: "openai",
  model: "gpt-4o",
  updated_at: "2026-10-06T12:00:00Z",
  worktree_branch: null,
  running: false,
  pending_permission_count: 0,
  ...overrides,
});

describe("sessionFilters persistence", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("loads empty arrays when localStorage is empty or corrupt", () => {
    expect(loadPinnedSessions()).toEqual([]);
    expect(loadArchivedSessions()).toEqual([]);

    localStorage.setItem("maukode.pinnedSessions", "invalid-json");
    expect(loadPinnedSessions()).toEqual([]);

    localStorage.setItem("maukode.archivedSessions", '{"not":"array"}');
    expect(loadArchivedSessions()).toEqual([]);
  });

  it("persists and dedupes pinned session IDs", () => {
    savePinnedSessions(["s1", "s2", "s1"]);
    expect(loadPinnedSessions()).toEqual(["s1", "s2"]);

    savePinnedSessions(["s3"]);
    expect(loadPinnedSessions()).toEqual(["s3"]);
  });

  it("persists and dedupes archived session IDs", () => {
    saveArchivedSessions(["s1", "s2", "s2"]);
    expect(loadArchivedSessions()).toEqual(["s1", "s2"]);

    saveArchivedSessions([]);
    expect(loadArchivedSessions()).toEqual([]);
  });
});

describe("projectOfSession", () => {
  it("prefers project_dir over directory when present", () => {
    expect(projectOfSession(mockSession({ directory: "/tmp/wt", project_dir: "/home/repo" }))).toBe(
      "/home/repo",
    );
    expect(projectOfSession(mockSession({ directory: "/home/repo", project_dir: null }))).toBe(
      "/home/repo",
    );
  });
});

describe("matchesSearch", () => {
  const session = mockSession({
    id: "s1",
    title: "Fix authentication race in background queue",
    directory: "/home/user/projects/maukode-desktop",
    project_dir: "/home/user/projects/maukode",
    worktree_branch: "maukode/feat-auth-race",
  });

  it("matches empty queries", () => {
    expect(matchesSearch(session, "")).toBe(true);
    expect(matchesSearch(session, "   ")).toBe(true);
  });

  it("matches session title (prompt / goal) case-insensitively", () => {
    expect(matchesSearch(session, "AUTH")).toBe(true);
    expect(matchesSearch(session, "background queue")).toBe(true);
    expect(matchesSearch(session, "database")).toBe(false);
  });

  it("matches repository name", () => {
    expect(matchesSearch(session, "maukode")).toBe(true);
  });

  it("matches directory path", () => {
    expect(matchesSearch(session, "projects")).toBe(true);
  });

  it("matches worktree branch", () => {
    expect(matchesSearch(session, "feat-auth-race")).toBe(true);
  });

  it("matches thread messages if available", () => {
    const thread: ThreadState = {
      ...emptyThread(),
      items: [
        { id: "1", kind: "user", text: "Please investigate SQLite checkpoint deadlock" },
        { id: "2", kind: "assistant", text: "I found the issue in transaction rollback." },
      ],
    };
    expect(matchesSearch(session, "sqlite", thread)).toBe(true);
    expect(matchesSearch(session, "transaction", thread)).toBe(true);
    expect(matchesSearch(session, "unrelated-term", thread)).toBe(false);
  });

  it("matches all tokens in multi-word search", () => {
    expect(matchesSearch(session, "maukode auth")).toBe(true);
    expect(matchesSearch(session, "maukode nonexisting")).toBe(false);
  });
});

describe("matchesStatus", () => {
  const idle = mockSession({ id: "idle", running: false, pending_permission_count: 0 });
  const running = mockSession({ id: "run", running: true, pending_permission_count: 0 });
  const needsYou = mockSession({ id: "needs", running: true, pending_permission_count: 2 });
  const archivedIdle = mockSession({ id: "archived", running: false, pending_permission_count: 0 });
  const archivedSet = new Set(["archived"]);

  it("filters for 'all' status (excludes archived)", () => {
    expect(matchesStatus(idle, "all", archivedSet)).toBe(true);
    expect(matchesStatus(running, "all", archivedSet)).toBe(true);
    expect(matchesStatus(needsYou, "all", archivedSet)).toBe(true);
    expect(matchesStatus(archivedIdle, "all", archivedSet)).toBe(false);
  });

  it("filters for 'needs_you' status", () => {
    expect(matchesStatus(needsYou, "needs_you", archivedSet)).toBe(true);
    expect(matchesStatus(idle, "needs_you", archivedSet)).toBe(false);
    expect(matchesStatus(running, "needs_you", archivedSet)).toBe(false);
  });

  it("filters for 'running' status", () => {
    expect(matchesStatus(running, "running", archivedSet)).toBe(true);
    expect(matchesStatus(needsYou, "running", archivedSet)).toBe(true);
    expect(matchesStatus(idle, "running", archivedSet)).toBe(false);
  });

  it("filters for 'completed' status", () => {
    expect(matchesStatus(idle, "completed", archivedSet)).toBe(true);
    expect(matchesStatus(running, "completed", archivedSet)).toBe(false);
    expect(matchesStatus(needsYou, "completed", archivedSet)).toBe(false);
  });

  it("filters for 'archived' status", () => {
    expect(matchesStatus(archivedIdle, "archived", archivedSet)).toBe(true);
    expect(matchesStatus(idle, "archived", archivedSet)).toBe(false);
  });
});

describe("getStatusCounts", () => {
  it("counts sessions accurately across all statuses", () => {
    const s1 = mockSession({ id: "s1", running: true, pending_permission_count: 0 });
    const s2 = mockSession({ id: "s2", running: true, pending_permission_count: 1 });
    const s3 = mockSession({ id: "s3", running: false, pending_permission_count: 0 });
    const s4 = mockSession({ id: "s4", running: false, pending_permission_count: 0 });
    const archivedIds = new Set(["s4"]);

    const counts = getStatusCounts([s1, s2, s3, s4], archivedIds);
    expect(counts).toEqual({
      all: 3,
      needs_you: 1,
      running: 2,
      completed: 1,
      archived: 1,
    });
  });
});

describe("sortSessions", () => {
  it("pins sessions to the top, sorted by updated_at within pinned and unpinned groups", () => {
    const s1 = mockSession({ id: "s1", updated_at: "2026-10-06T10:00:00Z" });
    const s2 = mockSession({ id: "s2", updated_at: "2026-10-06T12:00:00Z" });
    const s3 = mockSession({ id: "s3", updated_at: "2026-10-06T08:00:00Z" });
    const pinned = new Set(["s1", "s3"]);

    const sorted = sortSessions([s1, s2, s3], pinned);
    expect(sorted.map((s) => s.id)).toEqual(["s1", "s3", "s2"]);
  });
});

describe("filterSessions", () => {
  it("combines status filtering and search query", () => {
    const s1 = mockSession({ id: "s1", title: "Add auth login", running: true });
    const s2 = mockSession({ id: "s2", title: "Add export csv", running: false });
    const s3 = mockSession({ id: "s3", title: "Add auth logout", running: false });
    const archived = new Set<string>();

    const results = filterSessions([s1, s2, s3], {
      query: "auth",
      status: "completed",
      archivedIds: archived,
    });
    expect(results.map((s) => s.id)).toEqual(["s3"]);
  });
});

describe("groupSessionsByProject", () => {
  it("groups by project directory and sorts pinned sessions and projects first", () => {
    const p1s1 = mockSession({
      id: "p1s1",
      directory: "/code/proj1",
      updated_at: "2026-10-06T10:00:00Z",
    });
    const p1s2 = mockSession({
      id: "p1s2",
      directory: "/code/proj1",
      updated_at: "2026-10-06T09:00:00Z",
    });
    const p2s1 = mockSession({
      id: "p2s1",
      directory: "/code/proj2",
      updated_at: "2026-10-06T12:00:00Z",
    });
    const pinned = new Set(["p1s2"]);

    const groups = groupSessionsByProject([p1s1, p1s2, p2s1], pinned);

    // Project 1 has a pinned session, so Project 1 should come before Project 2
    expect(groups[0][0]).toBe("/code/proj1");
    // Inside Project 1, p1s2 is pinned, so it should be first
    expect(groups[0][1].map((s) => s.id)).toEqual(["p1s2", "p1s1"]);
    expect(groups[1][0]).toBe("/code/proj2");
  });
});
