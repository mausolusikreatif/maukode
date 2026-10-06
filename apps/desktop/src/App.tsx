import { useEffect, useState } from "react";
import type { PermissionMode } from "./api/types";
import { AuditDialog } from "./components/AuditDialog";
import { Composer } from "./components/Composer";
import { Dashboard } from "./components/Dashboard";
import { DeleteDialog } from "./components/DeleteDialog";
import { ConnectScreen } from "./components/ConnectScreen";
import { Prompts } from "./components/Prompts";
import { ReviewPanel } from "./components/ReviewPanel";
import { SettingsPage } from "./components/SettingsPage";
import { Sidebar } from "./components/Sidebar";
import { Thread } from "./components/Thread";
import { getBootstrap, type Bootstrap } from "./platform";
import { baseName } from "./state/format";
import { useApp } from "./state/store";
import { titleFor } from "./state/notify";
import { emptyThread } from "./state/thread";

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "ask", label: "Ask for risky actions" },
  { value: "strict", label: "Ask for everything" },
  { value: "review", label: "Review (private results)" },
  { value: "trust", label: "Trust (no prompts)" },
];

/** Approval mode for this session only; loads the server's current value when the session opens. */
function ModeSelect({ sessionId }: { sessionId: string }) {
  const client = useApp((s) => s.client);
  const [mode, setMode] = useState<PermissionMode | "">("");

  useEffect(() => {
    let cancelled = false;
    setMode("");
    void client
      ?.request<{ mode: PermissionMode }>("GET", `/api/v1/permissions/mode?session_id=${sessionId}`)
      .then((r) => !cancelled && setMode(r.mode))
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [client, sessionId]);

  return (
    <select
      data-testid="perm-mode"
      value={mode}
      disabled={!mode}
      onChange={(e) => {
        const next = e.target.value as PermissionMode;
        setMode(next);
        void client?.setPermissionMode(next, sessionId);
      }}
    >
      {MODES.map((m) => (
        <option key={m.value} value={m.value}>
          {m.label}
        </option>
      ))}
    </select>
  );
}

function SessionView() {
  const { activeId, meta, threads, review, toggleReview } = useApp();
  const [deleting, setDeleting] = useState(false);
  const [auditing, setAuditing] = useState(false);
  const view = useApp((s) => s.view);
  if (!activeId) return view === "settings" ? <SettingsPage /> : <Dashboard />;
  const thread = threads[activeId] ?? emptyThread();
  return (
    <div className={`session-layout ${review.open ? "with-review" : ""}`}>
      <main className="session-view">
        <header className="session-head">
          <div>
            <div className="crumbs" title={meta?.projectDir ?? meta?.directory}>
              <span className="chip project-chip">▤ {baseName(meta?.projectDir ?? meta?.directory ?? "")}</span>
              {meta?.branch && <span className="chip branch-chip">⎇ {meta.branch}</span>}
            </div>
            <strong data-testid="session-title">{meta?.title || "Session"}</strong>
            <span className="small muted">{meta?.provider}/{meta?.model}</span>
          </div>
          <div className="row">
            <ModeSelect sessionId={activeId} />
            <button data-testid="toggle-review" className={review.open ? "primary" : ""} onClick={() => void toggleReview()}>
              Changes{review.files.length > 0 ? ` (${review.files.length})` : ""}
            </button>
            <span data-testid="run-state" className={`badge ${thread.running ? "run" : ""}`}>
              {thread.running ? "running" : "idle"}
            </span>
            <button data-testid="audit-button" className="link" onClick={() => setAuditing(true)}>
              Audit
            </button>
            <button data-testid="delete-session" className="link" onClick={() => setDeleting(true)}>
              Delete
            </button>
          </div>
        </header>
        <Thread thread={thread} />
        <Prompts thread={thread} />
        <Composer running={thread.running} />
      </main>
      {review.open && <ReviewPanel running={thread.running} />}
      {auditing && <AuditDialog sessionId={activeId} onClose={() => setAuditing(false)} />}
      {deleting && <DeleteDialog sessionId={activeId} onClose={() => setDeleting(false)} />}
    </div>
  );
}

export default function App() {
  const { connection, connect, sessions } = useApp();
  const [boot, setBoot] = useState<Bootstrap | null>(null);
  const [ready, setReady] = useState(false);

  useEffect(() => {
    void getBootstrap()
      .then((b) => {
        setBoot(b);
        if (b) return connect(b.url, b.token);
      })
      .catch(() => undefined)
      .finally(() => setReady(true));
  }, [connect]);

  // Surface "N sessions need you" in the window/tab title.
  useEffect(() => {
    document.title = titleFor(sessions);
  }, [sessions]);

  if (!ready) return <div className="empty muted">Starting…</div>;
  if (connection === "disconnected" || connection === "connecting") {
    return <ConnectScreen initialUrl={boot?.url} initialToken={boot?.token} />;
  }
  return (
    <div className="app">
      <Sidebar />
      <SessionView />
    </div>
  );
}
