import type { ComponentType, ReactNode } from "react";
import {
  Activity,
  ArrowUpRight,
  Bot,
  CheckCircle2,
  Clock3,
  FolderGit2,
  Plus,
  ShieldAlert,
  Sparkles,
  TerminalSquare,
} from "lucide-react";
import { needsContinue } from "../api/types";
import type { SessionListItem } from "../api/types";
import { baseName } from "../state/format";
import { useApp } from "../state/store";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Skeleton } from "./ui/skeleton";

function ago(iso: string): string {
  const seconds = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000);
  if (seconds < 60) return "just now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)} min ago`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)} h ago`;
  return `${Math.floor(seconds / 86400)} d ago`;
}

function Stat({
  icon: Icon,
  label,
  value,
  tone = "neutral",
}: {
  icon: ComponentType<{ className?: string }>;
  label: string;
  value: number;
  tone?: "neutral" | "accent" | "warning";
}) {
  return (
    <div className={cn("stat-card rounded-xl border p-4", `stat-card-${tone}`)}>
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium uppercase tracking-[0.12em] text-muted-foreground">{label}</span>
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <strong className="mt-3 block text-2xl font-semibold tracking-tight">{value}</strong>
    </div>
  );
}

function Card({ s, note, tone }: { s: SessionListItem; note?: string; tone?: "warn" | "error" | "run" }) {
  const open = useApp((x) => x.openSession);
  const isAttention = tone === "warn" || tone === "error";

  return (
    <Button
      variant="ghost"
      data-testid="dash-card"
      className={cn(
        "dash-card group flex min-h-[116px] flex-col items-start gap-2 rounded-xl border bg-card/65 p-4 text-left transition-all hover:-translate-y-0.5 hover:border-primary/40 hover:bg-accent/40 hover:shadow-lg hover:shadow-black/10",
        tone && `dash-card-${tone}`,
        tone,
      )}
      onClick={() => void open(s.id)}
    >
      <div className="flex w-full items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <span
            className={cn(
              "mt-0.5 size-2 shrink-0 rounded-full bg-muted-foreground/40",
              tone === "run" && "animate-pulse bg-primary",
              tone === "warn" && "bg-warning",
              tone === "error" && "bg-destructive",
            )}
          />
          <span className="title truncate text-sm font-semibold">{s.title || "New Session"}</span>
        </div>
        <ArrowUpRight className="size-4 shrink-0 text-muted-foreground/60 transition-transform group-hover:-translate-y-0.5 group-hover:translate-x-0.5 group-hover:text-primary" />
      </div>
      <span className="flex items-center gap-1.5 truncate text-xs text-muted-foreground">
        <FolderGit2 className="size-3.5 shrink-0" />
        {baseName(s.project_dir ?? s.directory)}
        {s.worktree_branch && <span className="truncate">· ⎇ {s.worktree_branch}</span>}
      </span>
      {note ? (
        <span
          className={cn(
            "truncate text-xs",
            tone === "warn" && "text-warning",
            tone === "error" && "text-destructive",
            tone === "run" && "text-primary",
            !tone && "text-muted-foreground",
          )}
        >
          {note}
        </span>
      ) : (
        <span className="text-xs text-muted-foreground">{s.provider} · {s.model}</span>
      )}
      <span className="mt-auto flex items-center gap-1 text-xs text-muted-foreground/70">
        <Clock3 className="size-3" /> {ago(s.updated_at)}
      </span>
      {isAttention && <span className="sr-only">Needs attention</span>}
    </Button>
  );
}

function CardSkeleton() {
  return (
    <div className="flex min-h-[116px] flex-col gap-2 rounded-xl border border-border/40 bg-card/40 p-4">
      <div className="flex items-center gap-2">
        <Skeleton className="size-2 rounded-full" />
        <Skeleton className="h-4 w-32" />
      </div>
      <Skeleton className="h-3 w-48" />
      <Skeleton className="h-3 w-24" />
      <div className="mt-auto">
        <Skeleton className="h-3 w-16" />
      </div>
    </div>
  );
}

function Section({
  id,
  title,
  count,
  highlight,
  children,
}: {
  id: string;
  title: string;
  count: number;
  highlight?: boolean;
  children: ReactNode;
}) {
  if (count === 0) return null;
  return (
    <section data-testid={`dash-${id}`} className={cn("dash-section mt-8", highlight && "rounded-xl border border-primary/30 bg-primary/5 p-4")}>
      <div className="mb-3 flex items-center gap-2">
        {highlight && <span className="size-2 animate-pulse rounded-full bg-primary" />}
        <h3 className={cn("text-sm font-semibold tracking-tight", highlight && "text-primary")}>{title}</h3>
        <Badge variant="outline" className="badge rounded-full px-2 text-xs">
          {count}
        </Badge>
      </div>
      <div className="dash-grid grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">{children}</div>
    </section>
  );
}

/** What needs attention across every agent, most urgent first. */
export function Dashboard() {
  const { sessions, lastStop, connection } = useApp();
  const openNewSession = useApp((x) => x.openNewSession);
  const loading = connection === "connecting";

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
    <main data-testid="dashboard" className="dashboard dashboard-shell scrollbar-thin flex-1 overflow-y-auto px-4 py-6 safe-top safe-bottom lg:px-10">
      <div className="dashboard-glow" aria-hidden="true" />
      <header className="relative flex flex-col justify-between gap-5 border-b border-border/60 pb-7 sm:flex-row sm:items-end">
        <div>
          <div className="mb-3 flex items-center gap-2 text-xs font-medium uppercase tracking-[0.16em] text-primary">
            <Sparkles className="size-3.5" /> Agent workspace
          </div>
          <h1 className="text-2xl font-semibold tracking-[-0.03em] sm:text-3xl">Good to see you.</h1>
          <p className="mt-2 max-w-xl text-sm text-muted-foreground" data-testid="dash-summary">
            {needsYou.length ? `${needsYou.length} agent${needsYou.length === 1 ? " needs" : "s need"} your attention.` : "Your agents are ready when you are."}
            {running.length > 0 && ` ${running.length} currently working.`}
          </p>
        </div>
        <Button data-testid="dashboard-new-session" onClick={() => openNewSession()} size="lg">
          <Plus className="size-4" /> New session
        </Button>
      </header>

      <div className="relative mt-6 grid grid-cols-2 gap-3 lg:grid-cols-4">
        <Stat icon={Activity} label="Active" value={running.length} tone="accent" />
        <Stat icon={ShieldAlert} label="Needs you" value={needsYou.length} tone="warning" />
        <Stat icon={CheckCircle2} label="Completed" value={Math.max(0, sessions.length - running.length - needsYou.length - stopped.length)} />
        <Stat icon={Bot} label="All sessions" value={sessions.length} />
      </div>

      {sessions.length === 0 && (
        <div className="relative mt-8 flex flex-col items-center rounded-2xl border border-dashed border-border bg-card/30 px-6 py-16 text-center">
          <div className="mb-4 flex size-12 items-center justify-center rounded-2xl bg-primary/10 text-primary">
            <TerminalSquare className="size-6" />
          </div>
          <h2 className="text-base font-semibold">Start building with MauKode</h2>
          <p className="mt-2 max-w-sm text-sm text-muted-foreground">Give an agent a repo and a goal. It will plan, make changes, and leave every decision reviewable.</p>
          <Button variant="outline" className="mt-5" onClick={() => openNewSession()}>
            <Plus className="size-4" /> Create your first session
          </Button>
        </div>
      )}

      {loading && sessions.length === 0 && (
        <div className="relative mt-8 space-y-3">
          <Skeleton className="h-4 w-40" />
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
            {Array.from({ length: 3 }).map((_, i) => <CardSkeleton key={i} />)}
          </div>
        </div>
      )}

      <Section id="needs" title="Needs your attention" count={needsYou.length} highlight>
        {needsYou.map((s) => <Card key={s.id} s={s} tone="warn" note={`${s.pending_permission_count} approval${s.pending_permission_count === 1 ? "" : "s"} waiting`} />)}
      </Section>
      <Section id="stopped" title="Stopped early" count={stopped.length}>
        {stopped.map((s) => {
          const stop = lastStop[s.id]!;
          return <Card key={s.id} s={s} tone={stop.reason === "error" ? "error" : "warn"} note={stop.reason === "error" ? `Failed: ${stop.message ?? "unknown error"}` : (stop.message ?? "Stopped before finishing")} />;
        })}
      </Section>
      <Section id="running" title="Working now" count={running.length} highlight>
        {running.map((s) => <Card key={s.id} s={s} tone="run" note="Agent is working…" />)}
      </Section>
      <Section id="recent" title="Recent sessions" count={recent.length}>
        {recent.map((s) => <Card key={s.id} s={s} />)}
      </Section>
    </main>
  );
}
