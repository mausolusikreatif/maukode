import { needsContinue } from "../api/types";
import type { SessionListItem } from "../api/types";
import { baseName } from "../state/format";
import { useApp } from "../state/store";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";

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
    <button
      data-testid="dash-card"
      className={cn(
        "dash-card flex flex-col gap-1 rounded-lg border border-border bg-card/50 p-3 text-left transition-colors hover:bg-accent/40",
        tone && `dash-card-${tone}`,
        tone,
      )}
      onClick={() => void open(s.id)}
    >
      <span className="title truncate text-sm font-medium">{s.title || "New Session"}</span>
      <span className="text-xs text-muted-foreground">
        {baseName(s.project_dir ?? s.directory)}
        {s.worktree_branch && <> · ⎇ {s.worktree_branch}</>} · {s.model}
      </span>
      {note && (
        <span
          className={cn(
            "dash-note text-xs",
            tone === "warn" && "text-warning",
            tone === "error" && "text-destructive",
            tone === "run" && "text-primary",
            !tone && "text-muted-foreground",
          )}
        >
          {note}
        </span>
      )}
      <span className="text-xs text-muted-foreground">{ago(s.updated_at)}</span>
    </button>
  );
}

function Section({ id, title, count, children }: { id: string; title: string; count: number; children: React.ReactNode }) {
  if (count === 0) return null;
  return (
    <section data-testid={`dash-${id}`} className="dash-section mt-6">
      <h3 className="mb-2 flex items-center gap-2 text-sm font-semibold">
        {title}
        <Badge variant="outline" className="badge">
          {count}
        </Badge>
      </h3>
      <div className="dash-grid grid grid-cols-1 gap-2 sm:grid-cols-2 lg:grid-cols-3">{children}</div>
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
    <main data-testid="dashboard" className="dashboard scrollbar-thin flex-1 overflow-y-auto px-6 py-5">
      <h1 className="text-lg font-semibold tracking-tight">Overview</h1>
      <p className="mt-1 text-sm text-muted-foreground" data-testid="dash-summary">
        {needsYou.length} need you · {running.length} running · {stopped.length} stopped early · {sessions.length} total
      </p>
      {sessions.length === 0 && (
        <p className="mt-4 text-sm text-muted-foreground">No sessions yet. Create one from the sidebar.</p>
      )}

      <Section id="needs" title="Needs you" count={needsYou.length}>
        {needsYou.map((s) => (
          <Card
            key={s.id}
            s={s}
            tone="warn"
            note={`${s.pending_permission_count} approval${s.pending_permission_count === 1 ? "" : "s"} waiting`}
          />
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
        {recent.map((s) => <Card key={s.id} s={s} />)}
      </Section>
    </main>
  );
}
