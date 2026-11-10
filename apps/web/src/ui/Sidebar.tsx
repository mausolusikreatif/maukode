import { useMemo, useState } from "react";
import { useApp } from "../state/store";
import { baseName, shortAgo } from "../state/format";
import { CHAT_GROUP, filterSessions, groupSessionsByProject } from "../state/sessionFilters";
import type { SessionListItem } from "../api/types";

function stateOf(s: SessionListItem): { cls: string; label: string } {
  if (s.pending_permission_count > 0) return { cls: "hold", label: "waiting for permission" };
  if (s.running) return { cls: "run", label: "running" };
  return { cls: "", label: shortAgo(s.updated_at) };
}

export type Tab = "overview" | "sessions" | "new" | "settings";

export function Sidebar({ tab, onGo, onOpened }: {
  tab: Tab;
  onGo: (tab: Tab) => void;
  /** Called after a session is opened from the list. */
  onOpened: () => void;
}) {
  const sessions = useApp((s) => s.sessions);
  const activeId = useApp((s) => s.activeId);
  const threads = useApp((s) => s.threads);
  const query = useApp((s) => s.sessionSearchQuery);
  const pinned = useApp((s) => s.pinnedSessionIds);
  const archived = useApp((s) => s.archivedSessionIds);
  const [showArchived, setShowArchived] = useState(false);
  const connection = useApp((s) => s.connection);
  const inboxCount = useApp((s) => s.inbox.length);
  const { setSessionSearchQuery, openSession, togglePinSession, toggleArchiveSession, deleteSession } = useApp.getState();
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const groups = useMemo(() => {
    const filtered = filterSessions(sessions, {
      query,
      threads,
      status: showArchived ? "archived" : "all",
      archivedIds: new Set(archived),
    });
    return groupSessionsByProject(filtered, new Set(pinned));
  }, [sessions, query, threads, pinned, archived, showArchived]);

  const online = connection === "online";

  return (
    <>
      <aside className="side" aria-label="Sessions">
        <div className="head">
          <div className="head-title"><div className="t">MauKode</div></div>
          <span className="small muted" style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <span className={`dot ${online ? "on" : ""}`} />
            {online ? "connected" : connection === "reconnecting" ? "reconnecting" : "disconnected"}
          </span>
        </div>
        <div className="side-head">
          <button className="btn send desk-only" onClick={() => onGo("new")}>New session</button>
          <div className="side-nav">
            <button className="btn quiet small" aria-current={tab === "overview"} onClick={() => onGo("overview")}>
              Overview{inboxCount > 0 ? ` (${inboxCount})` : ""}
            </button>
            <button className="btn quiet small" aria-current={tab === "settings"} onClick={() => onGo("settings")}>Settings</button>
          </div>
          <input
            className="field" placeholder="Search sessions" value={query} aria-label="Search sessions"
            onChange={(e) => setSessionSearchQuery(e.target.value)}
          />
          {(archived.length > 0 || showArchived) && (
            <button className="btn quiet small" aria-pressed={showArchived} onClick={() => setShowArchived((v) => !v)}>
              {showArchived ? "Back to active sessions" : `Archive (${archived.length})`}
            </button>
          )}
        </div>
        <nav className="side-list">
          {groups.length === 0 && (
            <p className="muted small" style={{ padding: "1rem" }}>
              {sessions.length === 0
                ? "No sessions yet. Start with New session."
                : showArchived ? "No matching archived sessions." : "No matching sessions."}
            </p>
          )}
          {groups.map(([project, items]) => (
            <section key={project}>
              <div className="group-head">
                <span className="group-name trunc" title={project}>{baseName(project) || "No project"}</span>
                {project && (
                  <button
                    className="btn quiet small" aria-label={`New session in ${baseName(project)}`}
                    onClick={() => { useApp.getState().openNewSession(project === CHAT_GROUP ? "" : project); onGo("new"); }}
                  >
                    + New
                  </button>
                )}
              </div>
              {items.map((s) => {
                const st = stateOf(s);
                const isPinned = pinned.includes(s.id);
                return (
                  <div key={s.id}>
                    <button
                      className="sess" aria-current={s.id === activeId}
                      onClick={() => { void openSession(s.id); onOpened(); }}
                    >
                      <span className={`dot ${st.cls}`} />
                      <span style={{ minWidth: 0 }}>
                        <span className="sess-title trunc" style={{ display: "block" }}>
                          {isPinned ? "* " : ""}{s.title || "Untitled"}
                        </span>
                        <span className="sess-meta trunc" style={{ display: "block" }}>
                          {s.worktree_branch ? s.worktree_branch.replace(/^maukode\//, "") : s.model}
                        </span>
                      </span>
                      <span className="sess-meta">{st.label}</span>
                    </button>
                    {s.id === activeId && (
                      <div style={{ display: "flex", gap: 4, padding: "0 1rem 0.4rem 2.2rem" }}>
                        <button className="btn quiet small" onClick={() => togglePinSession(s.id)}>
                          {isPinned ? "Unpin" : "Pin"}
                        </button>
                        <button className="btn quiet small" onClick={() => void toggleArchiveSession(s.id)}>
                          {archived.includes(s.id) ? "Restore" : "Archive"}
                        </button>
                        {confirmId === s.id ? (
                          <button className="btn deny small" onClick={() => { setConfirmId(null); void deleteSession(s.id); }}>
                            Delete this session
                          </button>
                        ) : (
                          <button className="btn quiet small" onClick={() => setConfirmId(s.id)}>Delete</button>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </section>
          ))}
        </nav>
      </aside>
    </>
  );
}
