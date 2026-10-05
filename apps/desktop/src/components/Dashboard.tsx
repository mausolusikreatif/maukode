import { needsContinue } from "../api/types";
import type { SessionListItem } from "../api/types";
import { useApp } from "../state/store";

function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function ago(iso: string): string {
  const seconds = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}

function Card({ s, note, tone }: { s: SessionListItem; note?: string; tone?: "warn" | "error" | "run" }) {
  const open = useApp((x) => x.openSession);
  return (
    <button data-testid="dash-card" className={`dash-card ${tone ?? ""}`} onClick={() => void open(s.id)}>
      <span className="title">{s.title || "New Session"}</span>
      <span className="muted small">
        {baseName(s.project_dir ?? s.directory)}
        {s.worktree_branch && <> · ⎇ {s.worktree_branch}</>} · {s.model}
      </span>
      {note && <span className={`dash-note ${tone ?? ""}`}>{note}</span>}
      <span className="muted small">{ago(s.updated_at)}</span>
    </button>
  );
}

function Section({ id, title, count, children }: { id: string; title: string; count: number; children: React.ReactNode }) {
  if (count === 0) return null;
  return (
    <section data-testid={`dash-${id}`} className="dash-section">
      <h3>
        {title} <span className="badge">{count}</span>
      </h3>
      <div className="dash-grid">{children}</div>
    </section>
  );
}

/** What needs attention across every agent, most urgent first. */
export function Dashboard() {
  const { sessions, lastStop } = useApp();

  const needsYou = sessions.filter((s) => s.pending_permission_count > 0);
  const running = sessions.filter((s) => s.running && s.pending_permission_count === 0);
  const stopped = sessions.filter((s) => {
    const stop = lastStop[s.id];
    return !s.running && s.pending_permission_count === 0 && stop && (needsContinue(stop.reason) || stop.reason === "error");
  });
  const shown = new Set([...needsYou, ...running, ...stopped].map((s) => s.id));
  const recent = sessions
    .filter((s) => !shown.has(s.id))
    .sort((a, b) => b.updated_at.localeCompare(a.updated_at))
    .slice(0, 12);

  return (
    <main data-testid="dashboard" className="dashboard">
      <h1>Overview</h1>
      <p className="muted" data-testid="dash-summary">
        {needsYou.length} need you · {running.length} running · {stopped.length} stopped early · {sessions.length} total
      </p>
      {sessions.length === 0 && <p className="muted">No sessions yet. Create one from the sidebar.</p>}

      <Section id="needs" title="Needs you" count={needsYou.length}>
        {needsYou.map((s) => (
          <Card key={s.id} s={s} tone="warn" note={`${s.pending_permission_count} approval${s.pending_permission_count === 1 ? "" : "s"} waiting`} />
        ))}
      </Section>
      <Section id="stopped" title="Stopped early" count={stopped.length}>
        {stopped.map((s) => {
          const stop = lastStop[s.id]!;
          return (
            <Card
              key={s.id}
              s={s}
              tone={stop.reason === "error" ? "error" : "warn"}
              note={stop.reason === "error" ? `Failed: ${stop.message ?? "unknown error"}` : (stop.message ?? "Stopped before finishing")}
            />
          );
        })}
      </Section>
      <Section id="running" title="Running" count={running.length}>
        {running.map((s) => (
          <Card key={s.id} s={s} tone="run" note="Working…" />
        ))}
      </Section>
      <Section id="recent" title="Recent" count={recent.length}>
        {recent.map((s) => (
          <Card key={s.id} s={s} />
        ))}
      </Section>
    </main>
  );
}
