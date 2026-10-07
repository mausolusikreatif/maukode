import { useEffect, useState } from "react";
import { AuditDialog } from "./components/AuditDialog";
import { CommandPalette } from "./components/CommandPalette";
import { Composer } from "./components/Composer";
import { ConnectScreen } from "./components/ConnectScreen";
import { Dashboard } from "./components/Dashboard";
import { DeleteDialog } from "./components/DeleteDialog";
import { ModelSelect } from "./components/ModelSelect";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { Prompts } from "./components/Prompts";
import { ReviewPanel } from "./components/ReviewPanel";
import { SettingsPage } from "./components/SettingsPage";
import { ShortcutModal } from "./components/ShortcutModal";
import { Sidebar } from "./components/Sidebar";
import { Thread } from "./components/Thread";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import { getBootstrap, openInEditor, revealInFileManager, type Bootstrap } from "./platform";
import { baseName } from "./state/format";
import { useApp } from "./state/store";
import { titleFor } from "./state/notify";
import { emptyThread } from "./state/thread";
import { cn } from "./lib/utils";

function SessionTitle({ sessionId, title }: { sessionId: string; title: string | null }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(title || "");
  const renameSession = useApp((s) => s.renameSession);

  useEffect(() => {
    setValue(title || "");
  }, [title]);

  const commit = async () => {
    setEditing(false);
    if (value.trim() !== (title || "").trim()) {
      await renameSession(sessionId, value);
    }
  };

  if (editing) {
    return (
      <form
        className="title-edit-form flex items-center gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          void commit();
        }}
      >
        <input
          data-testid="session-title-input"
          className="title-input h-7 w-64 rounded-md border border-input bg-background/60 px-2 text-sm outline-none focus:ring-2 focus:ring-ring"
          autoFocus
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onBlur={() => void commit()}
        />
      </form>
    );
  }

  return (
    <button
      type="button"
      data-testid="session-title"
      className="title-text truncate rounded-md px-1 text-left text-sm font-medium hover:bg-accent"
      title="Rename session"
      onClick={() => setEditing(true)}
    >
      {title || "Untitled session"}
    </button>
  );
}

function SessionView() {
  const {
    activeId,
    meta,
    threads,
    sidebarOpen,
    review,
    archivedSessionIds,
    pinnedSessionIds,
    toggleSidebar,
    togglePinSession,
    toggleArchiveSession,
    toggleReview,
  } = useApp();
  const [deleting, setDeleting] = useState(false);
  const [auditing, setAuditing] = useState(false);
  const [toastMsg, setToastMsg] = useState<string | null>(null);
  const view = useApp((s) => s.view);
  if (!activeId) return view === "settings" ? <SettingsPage /> : <Dashboard />;
  const thread = threads[activeId] ?? emptyThread();
  const currentDir = meta?.directory ?? meta?.projectDir ?? "";

  const handleOpenEditor = async (editor: "code" | "cursor") => {
    if (!currentDir) return;
    const res = await openInEditor(currentDir, editor);
    if (!res.success && res.error) {
      setToastMsg(res.error);
      setTimeout(() => setToastMsg(null), 4000);
    }
  };

  const handleReveal = async () => {
    if (!currentDir) return;
    const res = await revealInFileManager(currentDir);
    if (!res.success && res.error) {
      setToastMsg(res.error);
      setTimeout(() => setToastMsg(null), 4000);
    }
  };

  const isPinned = pinnedSessionIds.includes(activeId);
  const isArchived = archivedSessionIds.includes(activeId);

  return (
    <div className="session-layout flex min-h-0 flex-1">
      <main className="session-view flex min-w-0 flex-1 flex-col">
        <header className="session-head flex flex-wrap items-center justify-between gap-3 border-b border-border/70 px-6 py-3">
          <div className="flex min-w-0 flex-col gap-1">
            <div className="crumbs-row flex items-center gap-2 text-xs text-muted-foreground">
              <div
                className="crumbs flex min-w-0 items-center gap-2"
                title={meta?.projectDir ?? meta?.directory}
              >
                {!sidebarOpen && (
                  <button
                    type="button"
                    className="toggle-sidebar-btn rounded-md px-1.5 py-0.5 hover:bg-accent"
                    data-testid="toggle-sidebar-btn"
                    title="Open Sidebar (Ctrl+B)"
                    onClick={toggleSidebar}
                  >
                    ☰
                  </button>
                )}
                <span className="project-chip truncate rounded-md bg-secondary px-2 py-0.5 font-mono text-[11px]">
                  ▤ {baseName(meta?.projectDir ?? meta?.directory ?? "")}
                </span>
                {meta?.branch && (
                  <span className="branch-chip rounded-md bg-secondary px-2 py-0.5 font-mono text-[11px]">
                    ⎇ {meta.branch}
                  </span>
                )}
                {isArchived && (
                  <span className="rounded-md bg-secondary px-2 py-0.5 font-mono text-[11px]">archived</span>
                )}
              </div>
              {currentDir && (
                <div className="repo-actions flex items-center gap-1" data-testid="repo-actions">
                  <button
                    type="button"
                    data-testid="open-vscode"
                    className="rounded-md border border-border px-2 py-0.5 text-[11px] hover:bg-accent"
                    title={`Open repository in VS Code (${currentDir})`}
                    onClick={() => void handleOpenEditor("code")}
                  >
                    VS Code
                  </button>
                  <button
                    type="button"
                    data-testid="open-cursor"
                    className="rounded-md border border-border px-2 py-0.5 text-[11px] hover:bg-accent"
                    title={`Open repository in Cursor (${currentDir})`}
                    onClick={() => void handleOpenEditor("cursor")}
                  >
                    Cursor
                  </button>
                  <button
                    type="button"
                    data-testid="reveal-file-manager"
                    className="rounded-md border border-border px-2 py-0.5 text-[11px] hover:bg-accent"
                    title={`Reveal repository in File Manager (${currentDir})`}
                    onClick={() => void handleReveal()}
                  >
                    Reveal
                  </button>
                </div>
              )}
            </div>
            <div className="flex items-center gap-3">
              <SessionTitle sessionId={activeId} title={meta?.title ?? null} />
              <ModelSelect
                sessionId={activeId}
                currentProvider={meta?.provider ?? null}
                currentModel={meta?.model ?? null}
              />
            </div>
          </div>
          <div className="flex flex-wrap items-center gap-3 text-xs">
            <Button
              data-testid="toggle-review"
              variant={review.open ? "default" : "outline"}
              size="sm"
              onClick={() => void toggleReview()}
            >
              Changes{review.files.length > 0 ? ` (${review.files.length})` : ""}
            </Button>
            <Badge
              data-testid="run-state"
              variant={thread.running ? "primary" : "outline"}
              className={cn("badge font-mono", thread.running && "run")}
            >
              {thread.running ? "running" : "idle"}
            </Badge>
            <button
              data-testid="pin-active-session"
              className="text-muted-foreground hover:text-foreground"
              title={isPinned ? "Unpin session" : "Pin session"}
              onClick={() => togglePinSession(activeId)}
            >
              {isPinned ? "Unpin" : "Pin"}
            </button>
            <button
              data-testid="archive-active-session"
              className="text-muted-foreground hover:text-foreground"
              title={isArchived ? "Restore session from archive" : "Archive session"}
              onClick={() => toggleArchiveSession(activeId)}
            >
              {isArchived ? "Restore" : "Archive"}
            </button>
            <button
              data-testid="audit-button"
              className="text-muted-foreground hover:text-foreground"
              onClick={() => setAuditing(true)}
            >
              Audit
            </button>
            <button
              data-testid="delete-session"
              className="text-muted-foreground hover:text-destructive"
              onClick={() => setDeleting(true)}
            >
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
      {toastMsg && (
        <div
          className="fixed bottom-5 left-1/2 z-[60] -translate-x-1/2 rounded-md border border-destructive/40 bg-popover px-4 py-2 text-sm text-destructive shadow-lg"
          data-testid="toast-message"
        >
          {toastMsg}
        </div>
      )}
    </div>
  );
}

export default function App() {
  const {
    connection,
    connect,
    sessions,
    sidebarOpen,
    newSessionOpen,
    newSessionInitialDir,
    closeNewSession,
    shortcutsOpen,
    commandPaletteOpen,
    toggleCommandPalette,
    closeCommandPalette,
    toggleShortcuts,
    closeShortcuts,
    toggleSidebar,
    openNewSession,
    toggleReview,
    stop,
    activeId,
  } = useApp();
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

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isCmdOrCtrl = e.metaKey || e.ctrlKey;
      const target = e.target as HTMLElement | null;
      const isInput =
        !!target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable);

      if (isCmdOrCtrl && e.key.toLowerCase() === "k") {
        e.preventDefault();
        toggleCommandPalette();
        return;
      }

      if (e.key === "Escape") {
        if (commandPaletteOpen) {
          e.preventDefault();
          closeCommandPalette();
          return;
        }
        if (shortcutsOpen) {
          e.preventDefault();
          closeShortcuts();
          return;
        }
        if (newSessionOpen) {
          e.preventDefault();
          closeNewSession();
          return;
        }
      }

      if ((isCmdOrCtrl && e.key === "/") || (!isInput && e.key === "?")) {
        e.preventDefault();
        toggleShortcuts();
        return;
      }

      if (isCmdOrCtrl && e.key.toLowerCase() === "b") {
        e.preventDefault();
        toggleSidebar();
        return;
      }

      if (isCmdOrCtrl && e.key.toLowerCase() === "n") {
        e.preventDefault();
        openNewSession(null);
        return;
      }

      if (isCmdOrCtrl && e.shiftKey && e.key.toLowerCase() === "d") {
        e.preventDefault();
        if (activeId) {
          void toggleReview();
        }
        return;
      }

      if (isCmdOrCtrl && e.key.toLowerCase() === "l") {
        e.preventDefault();
        const input = document.querySelector<HTMLTextAreaElement>(
          'textarea[data-testid="composer-input"]',
        );
        input?.focus();
        return;
      }

      if (isCmdOrCtrl && e.key === "Escape") {
        if (activeId) {
          e.preventDefault();
          void stop();
        }
        return;
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [
    commandPaletteOpen,
    shortcutsOpen,
    newSessionOpen,
    activeId,
    toggleCommandPalette,
    closeCommandPalette,
    toggleShortcuts,
    closeShortcuts,
    toggleSidebar,
    openNewSession,
    closeNewSession,
    toggleReview,
    stop,
  ]);

  if (!ready) return <div className="empty muted p-8 text-sm text-muted-foreground">Starting…</div>;
  if (connection === "disconnected" || connection === "connecting") {
    return <ConnectScreen initialUrl={boot?.url} initialToken={boot?.token} />;
  }
  return (
    <div className={cn("app flex h-screen w-screen overflow-hidden bg-background text-foreground")}>
      {sidebarOpen && <Sidebar />}
      <SessionView />
      {newSessionOpen && <NewSessionDialog initialDir={newSessionInitialDir} onClose={closeNewSession} />}
      {commandPaletteOpen && <CommandPalette />}
      {shortcutsOpen && <ShortcutModal />}
    </div>
  );
}
