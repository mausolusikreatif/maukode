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
import { getBootstrap, openInEditor, revealInFileManager, type Bootstrap } from "./platform";
import { baseName } from "./state/format";
import { useApp } from "./state/store";
import { titleFor } from "./state/notify";
import { emptyThread } from "./state/thread";

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
        className="title-edit-form"
        onSubmit={(e) => {
          e.preventDefault();
          void commit();
        }}
      >
        <input
          autoFocus
          className="title-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          onBlur={() => void commit()}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              setValue(title || "");
              setEditing(false);
            }
          }}
          placeholder="Session title…"
        />
      </form>
    );
  }

  return (
    <div className="title-row">
      <strong
        data-testid="session-title"
        className="title-text"
        title="Double click or click pencil to rename"
        onDoubleClick={() => setEditing(true)}
      >
        {title || "Session"}
      </strong>
      <button
        type="button"
        className="link icon-btn"
        title="Rename session"
        aria-label="Rename session"
        onClick={() => setEditing(true)}
      >
        ✎
      </button>
    </div>
  );
}

function SessionView() {
  const {
    activeId,
    meta,
    threads,
    review,
    toggleReview,
    sidebarOpen,
    toggleSidebar,
    pinnedSessionIds,
    archivedSessionIds,
    togglePinSession,
    toggleArchiveSession,
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
    <div className={`session-layout ${review.open ? "with-review" : ""}`}>
      <main className="session-view">
        <header className="session-head">
          <div>
            <div className="crumbs-row">
              <div className="crumbs" title={meta?.projectDir ?? meta?.directory}>
                {!sidebarOpen && (
                  <button
                    type="button"
                    className="link icon-btn toggle-sidebar-btn"
                    data-testid="toggle-sidebar-btn"
                    title="Open Sidebar (Ctrl+B)"
                    onClick={toggleSidebar}
                  >
                    ☰
                  </button>
                )}
                <span className="chip project-chip">▤ {baseName(meta?.projectDir ?? meta?.directory ?? "")}</span>
                {meta?.branch && <span className="chip branch-chip">⎇ {meta.branch}</span>}
                {isArchived && <span className="chip">archived</span>}
              </div>
              {currentDir && (
                <div className="repo-actions" data-testid="repo-actions">
                  <button
                    type="button"
                    data-testid="open-vscode"
                    className="btn-sm"
                    title={`Open repository in VS Code (${currentDir})`}
                    onClick={() => void handleOpenEditor("code")}
                  >
                    VS Code
                  </button>
                  <button
                    type="button"
                    data-testid="open-cursor"
                    className="btn-sm"
                    title={`Open repository in Cursor (${currentDir})`}
                    onClick={() => void handleOpenEditor("cursor")}
                  >
                    Cursor
                  </button>
                  <button
                    type="button"
                    data-testid="reveal-file-manager"
                    className="btn-sm"
                    title={`Reveal repository in File Manager (${currentDir})`}
                    onClick={() => void handleReveal()}
                  >
                    Reveal
                  </button>
                </div>
              )}
            </div>
            <SessionTitle sessionId={activeId} title={meta?.title ?? null} />
            <ModelSelect
              sessionId={activeId}
              currentProvider={meta?.provider ?? null}
              currentModel={meta?.model ?? null}
            />
          </div>
          <div className="row">
            <button data-testid="toggle-review" className={review.open ? "primary" : ""} onClick={() => void toggleReview()}>
              Changes{review.files.length > 0 ? ` (${review.files.length})` : ""}
            </button>
            <span data-testid="run-state" className={`badge ${thread.running ? "run" : ""}`}>
              {thread.running ? "running" : "idle"}
            </span>
            <button
              data-testid="pin-active-session"
              className="link"
              title={isPinned ? "Unpin session" : "Pin session"}
              onClick={() => togglePinSession(activeId)}
            >
              {isPinned ? "Unpin" : "Pin"}
            </button>
            <button
              data-testid="archive-active-session"
              className="link"
              title={isArchived ? "Restore session from archive" : "Archive session"}
              onClick={() => toggleArchiveSession(activeId)}
            >
              {isArchived ? "Restore" : "Archive"}
            </button>
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
      {toastMsg && (
        <div className="toast" data-testid="toast-message">
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

  // Global keyboard shortcuts
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const isCmdOrCtrl = e.metaKey || e.ctrlKey;
      const target = e.target as HTMLElement | null;
      const isInput =
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.tagName === "SELECT" ||
          target.isContentEditable);

      // Ctrl+K / Cmd+K: Command Palette
      if (isCmdOrCtrl && e.key.toLowerCase() === "k") {
        e.preventDefault();
        toggleCommandPalette();
        return;
      }

      // Escape: close open dialogs/palettes
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

      // Ctrl+/ / Cmd+/ or '?' (outside input): Keyboard Shortcuts Help
      if ((isCmdOrCtrl && e.key === "/") || (!isInput && e.key === "?")) {
        e.preventDefault();
        toggleShortcuts();
        return;
      }

      // Ctrl+B / Cmd+B: Toggle Sidebar
      if (isCmdOrCtrl && e.key.toLowerCase() === "b") {
        e.preventDefault();
        toggleSidebar();
        return;
      }

      // Ctrl+N / Cmd+N: New Task / Session
      if (isCmdOrCtrl && e.key.toLowerCase() === "n") {
        e.preventDefault();
        openNewSession(null);
        return;
      }

      // Ctrl+Shift+D / Cmd+Shift+D: Toggle Review Diff
      if (isCmdOrCtrl && e.shiftKey && e.key.toLowerCase() === "d") {
        e.preventDefault();
        if (activeId) {
          void toggleReview();
        }
        return;
      }

      // Ctrl+L / Cmd+L: Focus Composer
      if (isCmdOrCtrl && e.key.toLowerCase() === "l") {
        e.preventDefault();
        const input = document.querySelector<HTMLTextAreaElement>(
          'textarea[data-testid="composer-input"]'
        );
        input?.focus();
        return;
      }

      // Ctrl+Escape: Stop current task if running
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

  if (!ready) return <div className="empty muted">Starting…</div>;
  if (connection === "disconnected" || connection === "connecting") {
    return <ConnectScreen initialUrl={boot?.url} initialToken={boot?.token} />;
  }
  return (
    <div className={`app ${sidebarOpen ? "" : "sidebar-collapsed"}`}>
      {sidebarOpen && <Sidebar />}
      <SessionView />
      {newSessionOpen && (
        <NewSessionDialog
          initialDir={newSessionInitialDir}
          onClose={closeNewSession}
        />
      )}
      {commandPaletteOpen && <CommandPalette />}
      {shortcutsOpen && <ShortcutModal />}
    </div>
  );
}
