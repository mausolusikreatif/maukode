import { useEffect, useMemo, useRef, useState } from "react";
import { baseName, shortAgo } from "../state/format";
import {
  filterSessions,
  getStatusCounts,
  groupSessionsByProject,
  type SessionStatusFilter,
} from "../state/sessionFilters";
import { useApp } from "../state/store";
import { DeleteDialog } from "./DeleteDialog";
import { NewSessionDialog } from "./NewSessionDialog";

function PinIcon({ pinned }: { pinned: boolean }) {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill={pinned ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <line x1="12" y1="17" x2="12" y2="22" />
      <path d="M5 17h14v-1.76a2 2 0 0 0-1.11-1.79l-1.78-.89A2 2 0 0 1 15 10.77V5a1 1 0 0 1 1-1h1V2H7v2h1a1 1 0 0 1 1 1v5.77a2 2 0 0 1-1.11 1.79l-1.78.89A2 2 0 0 0 5 15.24Z" />
    </svg>
  );
}

function ArchiveIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="21 8 21 21 3 21 3 8" />
      <rect x="1" y="3" width="22" height="5" />
      <line x1="10" y1="12" x2="14" y2="12" />
    </svg>
  );
}

function RestoreIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <polyline points="1 4 1 10 7 10" />
      <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10" />
    </svg>
  );
}

function SearchIcon() {
  return (
    <svg
      width="13"
      height="13"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

const STATUS_FILTERS: { id: SessionStatusFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "needs_you", label: "Needs you" },
  { id: "running", label: "Running" },
  { id: "completed", label: "Completed" },
  { id: "archived", label: "Archived" },
];

export function Sidebar() {
  const {
    sessions,
    activeId,
    openSession,
    connection,
    view,
    goHome,
    openSettings,
    threads,
    sessionSearchQuery,
    sessionStatusFilter,
    pinnedSessionIds,
    archivedSessionIds,
    setSessionSearchQuery,
    setSessionStatusFilter,
    togglePinSession,
    toggleArchiveSession,
  } = useApp();

  const [creating, setCreating] = useState<string | null | false>(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    sessionId: string;
  } | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const toggle = (key: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  const pinnedSet = useMemo(() => new Set(pinnedSessionIds), [pinnedSessionIds]);
  const archivedSet = useMemo(() => new Set(archivedSessionIds), [archivedSessionIds]);

  const counts = useMemo(
    () => getStatusCounts(sessions, archivedSet),
    [sessions, archivedSet],
  );

  const filteredSessions = useMemo(
    () =>
      filterSessions(sessions, {
        query: sessionSearchQuery,
        status: sessionStatusFilter,
        archivedIds: archivedSet,
        threads,
      }),
    [sessions, sessionSearchQuery, sessionStatusFilter, archivedSet, threads],
  );

  const groups = useMemo(
    () => groupSessionsByProject(filteredSessions, pinnedSet),
    [filteredSessions, pinnedSet],
  );

  // Close context menu on outside pointerdown or Escape
  useEffect(() => {
    if (!contextMenu) return;
    const onPointerDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setContextMenu(null);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setContextMenu(null);
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [contextMenu]);

  const needsYou = counts.needs_you;
  const running = counts.running;

  return (
    <aside className="sidebar">
      <div className="sidebar-head">
        <strong>MauKode</strong>
        <span data-testid="connection" className={`dot ${connection}`} title={connection} />
      </div>

      <button data-testid="new-session" className="primary" onClick={() => setCreating(null)}>
        New session
      </button>

      <div className="summary muted" data-testid="summary">
        {running} running · {needsYou} need you
      </div>

      <div className="row nav">
        <button
          data-testid="nav-overview"
          className={!activeId && view === "home" ? "active" : ""}
          onClick={goHome}
        >
          Overview
        </button>
        <button
          data-testid="nav-settings"
          className={!activeId && view === "settings" ? "active" : ""}
          onClick={openSettings}
        >
          Settings
        </button>
      </div>

      {/* Session search input */}
      <div className="sidebar-search">
        <span className="search-icon" aria-hidden="true">
          <SearchIcon />
        </span>
        <input
          data-testid="session-search"
          type="search"
          placeholder="Search sessions…"
          value={sessionSearchQuery}
          onChange={(e) => setSessionSearchQuery(e.target.value)}
        />
        {sessionSearchQuery && (
          <button
            type="button"
            className="search-clear link"
            title="Clear search"
            aria-label="Clear search"
            onClick={() => setSessionSearchQuery("")}
          >
            ×
          </button>
        )}
      </div>

      {/* Status filter chips */}
      <div className="filter-chips" role="tablist" aria-label="Session filter">
        {STATUS_FILTERS.map((f) => {
          const count = counts[f.id];
          const isSelected = sessionStatusFilter === f.id;
          return (
            <button
              key={f.id}
              role="tab"
              data-testid={f.id === "needs_you" ? "filter-needs-you" : `filter-${f.id}`}
              aria-selected={isSelected}
              className={`filter-chip ${isSelected ? "active" : ""}`}
              onClick={() => setSessionStatusFilter(f.id)}
            >
              <span>{f.label}</span>
              {count > 0 && (
                <span
                  className={`chip-count ${
                    f.id === "needs_you" ? "warn" : f.id === "running" ? "run" : ""
                  }`}
                >
                  {count}
                </span>
              )}
            </button>
          );
        })}
      </div>

      <nav className="list">
        {groups.map(([project, items]) => {
          const closed = collapsed.has(project) && !items.some((s) => s.id === activeId);
          const attention = items.some((s) => s.pending_permission_count > 0);
          return (
            <section key={project} className="project">
              <div className="project-head" title={project}>
                <button
                  className="project-toggle"
                  onClick={() => toggle(project)}
                  aria-expanded={!closed}
                >
                  <span className={`chev ${closed ? "closed" : ""}`}>▾</span>
                  <span className="folder" aria-hidden="true">
                    ▤
                  </span>
                  <span className="project-name">{baseName(project)}</span>
                  {closed && attention && <span className="dot warn" />}
                  <span className="count">{items.length}</span>
                </button>
                <button
                  className="project-add"
                  title={`New session in ${baseName(project)}`}
                  aria-label={`New session in ${baseName(project)}`}
                  onClick={() => setCreating(project)}
                >
                  +
                </button>
              </div>

              {!closed &&
                items.map((s) => {
                  const isPinned = pinnedSet.has(s.id);
                  const isArchived = archivedSet.has(s.id);
                  return (
                    <div
                      key={s.id}
                      className={`session-row ${s.id === activeId ? "active" : ""}`}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setContextMenu({
                          x: Math.min(e.clientX, window.innerWidth - 180),
                          y: Math.min(e.clientY, window.innerHeight - 150),
                          sessionId: s.id,
                        });
                      }}
                    >
                      <button
                        data-testid="session-item"
                        className={`session ${s.id === activeId ? "active" : ""}`}
                        onClick={() => void openSession(s.id)}
                      >
                        <span className="title">
                          {isPinned && (
                            <span className="session-pin-indicator" title="Pinned" aria-label="Pinned">
                              📌
                            </span>
                          )}
                          {s.title || "New session"}
                        </span>
                        <span className="session-meta">
                          {s.pending_permission_count > 0 ? (
                            <span data-testid="needs-approval" className="badge warn">
                              {s.pending_permission_count} approval
                            </span>
                          ) : s.running ? (
                            <span data-testid="running-badge" className="badge run">
                              running
                            </span>
                          ) : (
                            <span>{shortAgo(s.updated_at)}</span>
                          )}
                          {s.worktree_branch && (
                            <span className="branch" title={s.worktree_branch}>
                              ⎇ {s.worktree_branch.replace(/^maukode\//, "")}
                            </span>
                          )}
                        </span>
                      </button>

                      <div className="session-actions">
                        <button
                          type="button"
                          data-testid="pin-session"
                          className={`session-action-btn ${isPinned ? "pinned" : ""}`}
                          title={isPinned ? "Unpin session" : "Pin session"}
                          aria-label={isPinned ? "Unpin session" : "Pin session"}
                          onClick={(e) => {
                            e.stopPropagation();
                            togglePinSession(s.id);
                          }}
                        >
                          <PinIcon pinned={isPinned} />
                        </button>
                        <button
                          type="button"
                          data-testid="archive-session"
                          className="session-action-btn"
                          title={isArchived ? "Restore session" : "Archive session"}
                          aria-label={isArchived ? "Restore session" : "Archive session"}
                          onClick={(e) => {
                            e.stopPropagation();
                            toggleArchiveSession(s.id);
                          }}
                        >
                          {isArchived ? <RestoreIcon /> : <ArchiveIcon />}
                        </button>
                      </div>
                    </div>
                  );
                })}
            </section>
          );
        })}

        {sessions.length === 0 && <p className="muted pad">No sessions yet.</p>}
        {sessions.length > 0 && filteredSessions.length === 0 && (
          <div className="empty-filter muted pad">
            <p>
              No{" "}
              {sessionStatusFilter === "all"
                ? ""
                : sessionStatusFilter === "needs_you"
                  ? "sessions needing approval"
                  : `${sessionStatusFilter} `}
              sessions{sessionSearchQuery ? ` matching “${sessionSearchQuery}”` : ""}.
            </p>
            {(sessionSearchQuery || sessionStatusFilter !== "all") && (
              <button
                type="button"
                className="link small"
                onClick={() => {
                  setSessionSearchQuery("");
                  setSessionStatusFilter("all");
                }}
              >
                Reset filters
              </button>
            )}
          </div>
        )}
      </nav>

      {/* Floating context menu */}
      {contextMenu && (
        <div
          ref={menuRef}
          className="session-context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          role="menu"
        >
          <button
            role="menuitem"
            data-testid="context-menu-pin"
            className="context-menu-item"
            onClick={() => {
              togglePinSession(contextMenu.sessionId);
              setContextMenu(null);
            }}
          >
            <span className="context-icon">
              <PinIcon pinned={pinnedSet.has(contextMenu.sessionId)} />
            </span>
            <span>{pinnedSet.has(contextMenu.sessionId) ? "Unpin session" : "Pin to top"}</span>
          </button>
          <button
            role="menuitem"
            data-testid="context-menu-archive"
            className="context-menu-item"
            onClick={() => {
              toggleArchiveSession(contextMenu.sessionId);
              setContextMenu(null);
            }}
          >
            <span className="context-icon">
              {archivedSet.has(contextMenu.sessionId) ? <RestoreIcon /> : <ArchiveIcon />}
            </span>
            <span>
              {archivedSet.has(contextMenu.sessionId) ? "Restore session" : "Archive session"}
            </span>
          </button>
          <hr className="context-divider" />
          <button
            role="menuitem"
            data-testid="context-menu-delete"
            className="context-menu-item danger"
            onClick={() => {
              const id = contextMenu.sessionId;
              setContextMenu(null);
              setDeletingId(id);
            }}
          >
            <span className="context-icon">✕</span>
            <span>Delete session…</span>
          </button>
        </div>
      )}

      {creating !== false && (
        <NewSessionDialog initialDir={creating} onClose={() => setCreating(false)} />
      )}
      {deletingId && (
        <DeleteDialog sessionId={deletingId} onClose={() => setDeletingId(null)} />
      )}
    </aside>
  );
}
