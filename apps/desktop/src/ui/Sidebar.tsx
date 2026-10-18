import { useMemo, useState } from "react";
import { useApp } from "../state/store";
import { baseName, shortAgo } from "../state/format";
import { filterSessions, groupSessionsByProject } from "../state/sessionFilters";
import type { SessionListItem } from "../api/types";

function stateOf(s: SessionListItem): { cls: string; label: string } {
  if (s.pending_permission_count > 0) return { cls: "hold", label: "menunggu izin" };
  if (s.running) return { cls: "run", label: "berjalan" };
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
  const connection = useApp((s) => s.connection);
  const inboxCount = useApp((s) => s.inbox.length);
  const { setSessionSearchQuery, openSession, togglePinSession, deleteSession } = useApp.getState();
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const groups = useMemo(() => {
    const filtered = filterSessions(sessions, { query, threads });
    return groupSessionsByProject(filtered, new Set(pinned));
  }, [sessions, query, threads, pinned]);

  const online = connection === "online";

  return (
    <>
      <aside className="side" aria-label="Sesi">
        <div className="head">
          <div className="head-title"><div className="t">MauKode</div></div>
          <span className="small muted" style={{ display: "flex", gap: 6, alignItems: "center" }}>
            <span className={`dot ${online ? "on" : ""}`} />
            {online ? "terhubung" : connection === "reconnecting" ? "menyambung ulang" : "terputus"}
          </span>
        </div>
        <div className="side-head">
          <button className="btn send desk-only" onClick={() => onGo("new")}>Sesi baru</button>
          <div className="side-nav">
            <button className="btn quiet small" aria-current={tab === "overview"} onClick={() => onGo("overview")}>
              Ringkasan{inboxCount > 0 ? ` (${inboxCount})` : ""}
            </button>
            <button className="btn quiet small" aria-current={tab === "settings"} onClick={() => onGo("settings")}>Pengaturan</button>
          </div>
          <input
            className="field" placeholder="Cari sesi" value={query} aria-label="Cari sesi"
            onChange={(e) => setSessionSearchQuery(e.target.value)}
          />
        </div>
        <nav className="side-list">
          {groups.length === 0 && (
            <p className="muted small" style={{ padding: "1rem" }}>
              {sessions.length === 0 ? "Belum ada sesi. Mulai dengan Sesi baru." : "Tidak ada sesi yang cocok."}
            </p>
          )}
          {groups.map(([project, items]) => (
            <section key={project}>
              <div className="group-head">
                <span className="group-name trunc" title={project}>{baseName(project) || "Tanpa proyek"}</span>
                {project && (
                  <button
                    className="btn quiet small" aria-label={`Sesi baru di ${baseName(project)}`}
                    onClick={() => { useApp.getState().openNewSession(project); onGo("new"); }}
                  >
                    + Baru
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
                          {isPinned ? "* " : ""}{s.title || "Tanpa judul"}
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
                          {isPinned ? "Lepas sematan" : "Sematkan"}
                        </button>
                        {confirmId === s.id ? (
                          <button className="btn deny small" onClick={() => { setConfirmId(null); void deleteSession(s.id); }}>
                            Hapus sesi ini
                          </button>
                        ) : (
                          <button className="btn quiet small" onClick={() => setConfirmId(s.id)}>Hapus</button>
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
