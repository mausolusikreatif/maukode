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

export function Sidebar({ open, onClose, theme, onTheme }: {
  open: boolean;
  onClose: () => void;
  theme: string;
  onTheme: () => void;
}) {
  const sessions = useApp((s) => s.sessions);
  const activeId = useApp((s) => s.activeId);
  const threads = useApp((s) => s.threads);
  const query = useApp((s) => s.sessionSearchQuery);
  const pinned = useApp((s) => s.pinnedSessionIds);
  const connection = useApp((s) => s.connection);
  const { setSessionSearchQuery, openSession, goHome, togglePinSession, deleteSession, disconnect } = useApp.getState();
  const [confirmId, setConfirmId] = useState<string | null>(null);

  const groups = useMemo(() => {
    const filtered = filterSessions(sessions, { query, threads });
    return groupSessionsByProject(filtered, new Set(pinned));
  }, [sessions, query, threads, pinned]);

  const online = connection === "online";

  return (
    <>
      <div className={`scrim ${open ? "open" : ""}`} onClick={onClose} />
      <aside className={`side ${open ? "open" : ""}`} aria-label="Sesi">
        <div className="side-head">
          <div className="brand">
            <span className="brand-name">MauKode</span>
            <span className="small muted" style={{ display: "flex", gap: 6, alignItems: "center" }}>
              <span className={`dot ${online ? "on" : ""}`} />
              {online ? "terhubung" : connection === "reconnecting" ? "menyambung ulang" : "terputus"}
            </span>
          </div>
          <button className="btn send" onClick={() => { goHome(); onClose(); }}>Sesi baru</button>
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
              <div className="group-name trunc" title={project}>{baseName(project)}</div>
              {items.map((s) => {
                const st = stateOf(s);
                const isPinned = pinned.includes(s.id);
                return (
                  <div key={s.id}>
                    <button
                      className="sess" aria-current={s.id === activeId}
                      onClick={() => { void openSession(s.id); onClose(); }}
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
        <div className="side-foot">
          <button className="btn quiet small" onClick={onTheme}>Tema: {theme === "system" ? "otomatis" : theme === "light" ? "terang" : "gelap"}</button>
          <button className="btn quiet small" onClick={() => disconnect()}>Putus</button>
        </div>
      </aside>
    </>
  );
}
