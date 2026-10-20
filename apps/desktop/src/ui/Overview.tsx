import { useEffect, useMemo, useState } from "react";
import { useApp } from "../state/store";
import { baseName, shortAgo } from "../state/format";
import { projectOfSession } from "../state/sessionFilters";
import type { PermissionRequest, SessionListItem, StopReason } from "../api/types";

const STOP_LABEL: Record<StopReason, string> = {
  completed: "selesai",
  max_tokens: "batas token",
  max_steps: "batas langkah",
  cancelled: "dihentikan",
  error: "error",
};

/** "12m", "1j 5m": how long a turn has been running. */
function elapsed(iso: string | null): string {
  if (!iso) return "";
  const s = Math.max(0, Math.floor((Date.now() - new Date(iso).getTime()) / 1000));
  if (s < 60) return `${s}d`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m}m` : `${Math.floor(m / 60)}j ${m % 60}m`;
}

function useTick(ms: number) {
  const [, set] = useState(0);
  useEffect(() => {
    const t = setInterval(() => set((n) => n + 1), ms);
    return () => clearInterval(t);
  }, [ms]);
}

function InboxCard({ req, title }: { req: PermissionRequest; title: string }) {
  const { respondTo, openSession } = useApp.getState();
  const detail = req.command ?? req.path ?? req.description;
  return (
    <section className="inbox-card">
      <div className="dock-top">
        <span className="strong trunc">{title}</span>
        <span className="small muted">
          {req.tool_name}, risiko <span className="risk" data-r={req.risk_level}>{req.risk_level}</span>
        </span>
      </div>
      <pre className="cmd">{detail}</pre>
      <div className="acts">
        <button className="btn deny" onClick={() => void respondTo(req.session_id, req.request_id, "deny")}>Tolak</button>
        <button className="btn ink" onClick={() => void respondTo(req.session_id, req.request_id, "allow_once")}>Izinkan sekali</button>
      </div>
      <details className="more">
        <summary>Opsi lain</summary>
        <div className="acts2">
          <button className="btn" onClick={() => void respondTo(req.session_id, req.request_id, "allow_session")}>Izinkan untuk sesi ini</button>
          <button className="btn" onClick={() => void respondTo(req.session_id, req.request_id, "allow_private")}>Jalankan privat</button>
          <button className="btn quiet" onClick={() => void openSession(req.session_id)}>Buka sesi</button>
        </div>
      </details>
    </section>
  );
}

function Row({ s, note }: { s: SessionListItem; note?: string }) {
  const hasPlan = s.plan_total > 0;
  return (
    <button className="orow" onClick={() => void useApp.getState().openSession(s.id)}>
      <span className="orow-top">
        <span className="strong trunc">{s.title || "Tanpa judul"}</span>
        <span className="small muted">{s.running ? elapsed(s.turn_started_at) : note ?? shortAgo(s.updated_at)}</span>
      </span>
      <span className="small muted trunc">
        {baseName(projectOfSession(s)) || "Tanpa proyek"}{s.worktree_branch ? ` / ${s.worktree_branch.replace(/^maukode\//, "")}` : ""}
      </span>
      {s.running && hasPlan && (
        <span className="prog" role="progressbar" aria-valuemin={0} aria-valuemax={s.plan_total} aria-valuenow={s.plan_done} aria-label="Progres rencana">
          <span style={{ width: `${(s.plan_done / s.plan_total) * 100}%` }} />
        </span>
      )}
      {s.running && (
        <span className="small trunc">
          {hasPlan ? `langkah ${s.plan_done}/${s.plan_total}` : "bekerja"}
          {s.last_activity ? `, ${s.last_activity}` : ""}
        </span>
      )}
    </button>
  );
}

export function Overview({ onNew }: { onNew: () => void }) {
  const sessions = useApp((s) => s.sessions);
  const inbox = useApp((s) => s.inbox);
  const lastStop = useApp((s) => s.lastStop);
  const [project, setProject] = useState<string | null>(null);
  useTick(15000);

  const anyRunning = sessions.some((s) => s.running);
  useEffect(() => {
    if (!anyRunning) return;
    const t = setInterval(() => void useApp.getState().refreshSessions(), 5000);
    return () => clearInterval(t);
  }, [anyRunning]);

  const projects = useMemo(() => [...new Set(sessions.map(projectOfSession))], [sessions]);
  const visible = useMemo(
    () => sessions.filter((s) => project === null || projectOfSession(s) === project),
    [sessions, project],
  );
  const titleOf = (id: string) => sessions.find((s) => s.id === id)?.title || "Tanpa judul";
  const waiting = inbox.filter((r) => visible.some((s) => s.id === r.session_id));
  const running = visible.filter((s) => s.running);
  // Archived sessions still show while they run or wait for approval, not among the recent ones.
  const recent = visible
    .filter((s) => !s.running && !s.archived)
    .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
    .slice(0, 8);

  return (
    <>
      <header className="head"><div className="head-title"><div className="t">Ringkasan</div></div></header>
      <div className="home">
      <div className="home-in wide">

        {projects.length > 1 && (
          <div className="chips" role="group" aria-label="Filter proyek">
            <button className="btn small" aria-pressed={project === null} onClick={() => setProject(null)}>Semua</button>
            {projects.map((p) => (
              <button key={p} className="btn small" aria-pressed={project === p} title={p} onClick={() => setProject(p)}>
                {baseName(p) || "Tanpa proyek"}
              </button>
            ))}
          </div>
        )}

        <section aria-label="Perlu keputusan">
          <h2 className="osec hold">Perlu keputusan Anda{waiting.length ? ` (${waiting.length})` : ""}</h2>
          {waiting.length === 0 ? (
            <p className="muted small">Tidak ada agent yang menunggu izin.</p>
          ) : (
            <div className="stack">{waiting.map((r) => <InboxCard key={r.request_id} req={r} title={titleOf(r.session_id)} />)}</div>
          )}
        </section>

        <section aria-label="Sedang berjalan">
          <h2 className="osec">Sedang berjalan{running.length ? ` (${running.length})` : ""}</h2>
          {running.length === 0 ? (
            <p className="muted small">Tidak ada agent yang bekerja sekarang.</p>
          ) : (
            <div className="list">{running.map((s) => <Row key={s.id} s={s} />)}</div>
          )}
        </section>

        <section aria-label="Baru selesai">
          <h2 className="osec">Aktivitas terakhir</h2>
          {recent.length === 0 ? (
            <div className="stack">
              <p className="muted small">Belum ada sesi.</p>
              <button className="btn send" style={{ justifySelf: "start" }} onClick={onNew}>Mulai sesi baru</button>
            </div>
          ) : (
            <div className="list">
              {recent.map((s) => (
                <Row key={s.id} s={s} note={lastStop[s.id] ? `${STOP_LABEL[lastStop[s.id].reason]}, ${shortAgo(s.updated_at)}` : undefined} />
              ))}
            </div>
          )}
        </section>
      </div>
      </div>
    </>
  );
}
