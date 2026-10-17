import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Eye, Folder, GitBranch, MoreHorizontal, PanelLeft, Pin, Archive, ShieldCheck, Trash2 } from "lucide-react";
import { AuditDialog } from "./components/AuditDialog";
import { CommandPalette } from "./components/CommandPalette";
import { Composer } from "./components/Composer";
import { ConnectScreen } from "./components/ConnectScreen";
import { Dashboard } from "./components/Dashboard";
import { DeleteDialog } from "./components/DeleteDialog";
import { ModelSelect } from "./components/ModelSelect";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { Prompts } from "./components/Prompts";
import { PreviewPanel } from "./components/PreviewPanel";
import { ReviewPanel } from "./components/ReviewPanel";
import { SettingsPage } from "./components/SettingsPage";
import { ShortcutModal } from "./components/ShortcutModal";
import { Sidebar } from "./components/Sidebar";
import { Thread } from "./components/Thread";
import { Badge } from "./components/ui/badge";
import { Button } from "./components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "./components/ui/dropdown-menu";
import { getBootstrap, openInEditor, revealInFileManager, type Bootstrap } from "./platform";
import { baseName } from "./state/format";
import { useApp } from "./state/store";
import { titleFor } from "./state/notify";
import { emptyThread } from "./state/thread";
import { useIsMobile } from "./hooks/useIsMobile";
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
    <Button
      variant="ghost"
      size="sm"
      data-testid="session-title"
      className="title-text h-auto truncate justify-start px-1 text-left text-sm font-medium"
      title="Rename session"
      onClick={() => setEditing(true)}
    >
      {title || "Untitled session"}
    </Button>
  );
}

function SessionView() {
  const {
    activeId,
    meta,
    threads,
    sidebarOpen,
    review,
    preview,
    archivedSessionIds,
    pinnedSessionIds,
    toggleSidebar,
    togglePinSession,
    toggleArchiveSession,
    toggleReview,
    togglePreview,
  } = useApp();
  const isMobile = useIsMobile();
  const [deleting, setDeleting] = useState(false);
  const [auditing, setAuditing] = useState(false);
  const view = useApp((s) => s.view);
  if (!activeId) return view === "settings" ? <SettingsPage /> : <Dashboard />;
  const thread = threads[activeId] ?? emptyThread();
  const currentDir = meta?.directory ?? meta?.projectDir ?? "";

  const handleOpenEditor = async (editor: "code" | "cursor") => {
    if (!currentDir) return;
    const res = await openInEditor(currentDir, editor);
    if (!res.success && res.error) toast.error(res.error);
  };

  const handleReveal = async () => {
    if (!currentDir) return;
    const res = await revealInFileManager(currentDir);
    if (!res.success && res.error) toast.error(res.error);
  };

  const isPinned = pinnedSessionIds.includes(activeId);
  const isArchived = archivedSessionIds.includes(activeId);

  return (
    <div className="session-layout flex min-h-0 min-w-0 flex-1 overflow-hidden">
      <main className="session-view flex min-w-0 flex-1 flex-col overflow-hidden bg-background">
        <header
          className={cn(
            "session-head flex min-w-0 items-center justify-between gap-2 border-b border-border/70 bg-background/95 backdrop-blur",
            isMobile ? "px-3 py-2.5 safe-top" : "min-h-[68px] flex-wrap gap-3 px-5 py-3.5",
          )}
        >
          <div className="flex min-w-0 flex-1 items-center gap-2">
            {(!sidebarOpen || isMobile) && (
              <Button
                variant="ghost"
                size="icon"
                data-testid="toggle-sidebar-btn"
                title="Open Sidebar (Ctrl+B)"
                className="touch-target"
                onClick={toggleSidebar}
              >
                <PanelLeft className="size-4" />
              </Button>
            )}
            <div className="flex min-w-0 flex-col gap-0.5">
              <div className="flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground">
                <Folder className="size-3 shrink-0" />
                <span className="truncate font-medium text-foreground" title={meta?.projectDir ?? meta?.directory}>
                  {baseName(meta?.projectDir ?? meta?.directory ?? "")}
                </span>
                {meta?.branch && <><span className="text-border">/</span><span className="flex items-center gap-0.5 truncate"><GitBranch className="size-3" />{meta.branch}</span></>}
              </div>
              {!isMobile && (
                <div className="flex items-center gap-3">
                  <SessionTitle sessionId={activeId} title={meta?.title ?? null} />
                  <ModelSelect sessionId={activeId} currentProvider={meta?.provider ?? null} currentModel={meta?.model ?? null} />
                </div>
              )}
              {isMobile && (
                <SessionTitle sessionId={activeId} title={meta?.title ?? null} />
              )}
            </div>
          </div>
          <div className={cn("flex shrink-0 items-center gap-1 text-xs", !isMobile && "flex-wrap gap-2")}>
            <Badge data-testid="run-state" variant={thread.running ? "primary" : "outline"} className={cn("badge font-mono", thread.running && "run")}>
              {thread.running ? "running" : "idle"}
            </Badge>
            {!isMobile && (
              <Button data-testid="toggle-review" variant={review.open ? "default" : "outline"} size="sm" className="h-8" onClick={() => void toggleReview()}>
                Changes{review.files.length > 0 ? ` (${review.files.length})` : ""}
              </Button>
            )}
            {isMobile && review.files.length > 0 && (
              <Button data-testid="toggle-review" variant={review.open ? "default" : "outline"} size="sm" className="h-8 px-2" onClick={() => void toggleReview()}>
                {review.files.length}
              </Button>
            )}
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="ghost"
                  size="icon"
                  aria-label="Session actions"
                  title="Session actions"
                  data-testid="session-actions"
                  className="touch-target"
                >
                  <MoreHorizontal className="size-4" />
                </Button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-48">
                <DropdownMenuItem data-testid="open-vscode" onClick={() => void handleOpenEditor("code")}>
                  <Folder className="size-3.5" /> Open in VS Code
                </DropdownMenuItem>
                <DropdownMenuItem data-testid="open-cursor" onClick={() => void handleOpenEditor("cursor")}>
                  <Folder className="size-3.5" /> Open in Cursor
                </DropdownMenuItem>
                <DropdownMenuItem data-testid="reveal-file-manager" onClick={() => void handleReveal()}>
                  <Folder className="size-3.5" /> Reveal folder
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem data-testid="pin-active-session" onClick={() => togglePinSession(activeId)}>
                  <Pin className="size-3.5" fill={isPinned ? "currentColor" : "none"} />
                  {isPinned ? "Unpin session" : "Pin session"}
                </DropdownMenuItem>
                <DropdownMenuItem data-testid="archive-active-session" onClick={() => toggleArchiveSession(activeId)}>
                  <Archive className="size-3.5" /> {isArchived ? "Restore" : "Archive"}
                </DropdownMenuItem>
                {isMobile && (
                  <>
                    <DropdownMenuSeparator />
                    <DropdownMenuItem onClick={() => void togglePreview()}>
                      <Eye className="size-3.5" /> {preview.open ? "Hide preview" : "Preview"}
                    </DropdownMenuItem>
                    <DropdownMenuItem onClick={() => void toggleReview()}>
                      <Folder className="size-3.5" /> Changes
                    </DropdownMenuItem>
                  </>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
            {!isMobile && (
              <Button variant="ghost" size="icon" data-testid="audit-button" className="touch-target" title="Audit session" onClick={() => setAuditing(true)}>
                <ShieldCheck className="size-4" />
              </Button>
            )}
            {!isMobile && (
              <Button variant="ghost" size="icon" data-testid="preview-toggle" className={cn("touch-target", preview.open && "bg-primary/15 text-primary")} title="Toggle preview" onClick={togglePreview}>
                <Eye className="size-4" />
              </Button>
            )}
            <Button variant="ghost" size="icon" data-testid="delete-session" className="touch-target hover:bg-destructive/10 hover:text-destructive" title="Delete session" onClick={() => setDeleting(true)}>
              <Trash2 className="size-4" />
            </Button>
          </div>
        </header>
        <Thread thread={thread} isMobile={isMobile} />
        <Prompts thread={thread} />
        <Composer running={thread.running} isMobile={isMobile} />
      </main>
      {review.open && <ReviewPanel running={thread.running} />}
      {preview.open && activeId && <PreviewPanel sessionId={activeId} />}
      {auditing && <AuditDialog sessionId={activeId} onClose={() => setAuditing(false)} />}
      {deleting && <DeleteDialog sessionId={activeId} onClose={() => setDeleting(false)} />}
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
  const isMobile = useIsMobile();

  // Close sidebar on mobile when app becomes ready (avoid initial overlay)
  useEffect(() => {
    if (ready && isMobile && sidebarOpen) {
      toggleSidebar();
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, isMobile]);

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
