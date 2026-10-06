import { useMemo, useState } from "react";
import type { SessionListItem } from "../api/types";
import { baseName, shortAgo } from "../state/format";
import { useApp } from "../state/store";
import { NewSessionDialog } from "./NewSessionDialog";

function projectOf(s: SessionListItem): string {
  return s.project_dir ?? s.directory;
}

export function Sidebar() {
  const { sessions, activeId, openSession, connection, view, goHome, openSettings } = useApp();
  const [creating, setCreating] = useState<string | null | false>(false);
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const toggle = (key: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  const groups = useMemo(() => {
    const map = new Map<string, SessionListItem[]>();
    for (const s of sessions) {
      const key = projectOf(s);
      map.set(key, [...(map.get(key) ?? []), s]);
    }
    return [...map.entries()];
  }, [sessions]);

  const needsYou = sessions.filter((s) => s.pending_permission_count > 0).length;
  const running = sessions.filter((s) => s.running).length;

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
        <button data-testid="nav-overview" className={!activeId && view === "home" ? "active" : ""} onClick={goHome}>
          Overview
        </button>
        <button data-testid="nav-settings" className={!activeId && view === "settings" ? "active" : ""} onClick={openSettings}>
          Settings
        </button>
      </div>
      <nav className="list">
        {groups.map(([project, items]) => {
          const closed = collapsed.has(project) && !items.some((s) => s.id === activeId);
          const attention = items.some((s) => s.pending_permission_count > 0);
          return (
            <section key={project} className="project">
              <div className="project-head" title={project}>
                <button className="project-toggle" onClick={() => toggle(project)} aria-expanded={!closed}>
                  <span className={`chev ${closed ? "closed" : ""}`}>▾</span>
                  <span className="folder" aria-hidden>▤</span>
                  <span className="project-name">{baseName(project)}</span>
                  {closed && attention && <span className="dot warn" />}
                  <span className="count">{items.length}</span>
                </button>
                <button className="project-add" title={`New session in ${baseName(project)}`} aria-label={`New session in ${baseName(project)}`} onClick={() => setCreating(project)}>
                  +
                </button>
              </div>
              {!closed &&
                items.map((s) => (
                  <button
                    key={s.id}
                    data-testid="session-item"
                    className={`session ${s.id === activeId ? "active" : ""}`}
                    onClick={() => void openSession(s.id)}
                  >
                    <span className="title">{s.title || "New session"}</span>
                    <span className="session-meta">
                      {s.pending_permission_count > 0 ? (
                        <span data-testid="needs-approval" className="badge warn">
                          {s.pending_permission_count} approval
                        </span>
                      ) : s.running ? (
                        <span data-testid="running-badge" className="badge run">running</span>
                      ) : (
                        <span>{shortAgo(s.updated_at)}</span>
                      )}
                      {s.worktree_branch && (
                        <span className="branch" title={s.worktree_branch}>⎇ worktree</span>
                      )}
                    </span>
                  </button>
                ))}
            </section>
          );
        })}
        {sessions.length === 0 && <p className="muted pad">No sessions yet.</p>}
      </nav>
      {creating !== false && <NewSessionDialog initialDir={creating} onClose={() => setCreating(false)} />}
    </aside>
  );
}
