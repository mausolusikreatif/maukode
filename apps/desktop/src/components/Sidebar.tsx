import { useMemo, useState } from "react";
import type { SessionListItem } from "../api/types";
import { useApp } from "../state/store";
import { NewSessionDialog } from "./NewSessionDialog";

function projectOf(s: SessionListItem): string {
  return s.project_dir ?? s.directory;
}

function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function Sidebar() {
  const { sessions, activeId, openSession, connection } = useApp();
  const [creating, setCreating] = useState(false);

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
      <button data-testid="new-session" className="primary" onClick={() => setCreating(true)}>
        New session
      </button>
      <div className="summary muted" data-testid="summary">
        {running} running · {needsYou} need you
      </div>
      <nav>
        {groups.map(([project, items]) => (
          <section key={project}>
            <h3 title={project}>{baseName(project)}</h3>
            {items.map((s) => (
              <button
                key={s.id}
                data-testid="session-item"
                className={`session ${s.id === activeId ? "active" : ""}`}
                onClick={() => void openSession(s.id)}
              >
                <span className="title">{s.title || "New Session"}</span>
                <span className="badges">
                  {s.worktree_branch && <span className="badge" title={s.worktree_branch}>⎇</span>}
                  {s.pending_permission_count > 0 && (
                    <span data-testid="needs-approval" className="badge warn">
                      {s.pending_permission_count} approval
                    </span>
                  )}
                  {s.running && <span data-testid="running-badge" className="badge run">running</span>}
                </span>
              </button>
            ))}
          </section>
        ))}
        {sessions.length === 0 && <p className="muted pad">No sessions yet.</p>}
      </nav>
      {creating && <NewSessionDialog onClose={() => setCreating(false)} />}
    </aside>
  );
}
