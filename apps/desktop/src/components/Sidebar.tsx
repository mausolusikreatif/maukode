import { useEffect, useMemo, useRef, useState } from "react";
import {
  Archive,
  ArchiveRestore,
  ChevronDown,
  CircleHelp,
  House,
  Pin,
  Plus,
  Search,
  Settings2,
  Trash2,
  X,
} from "lucide-react";
import { baseName, shortAgo } from "../state/format";
import {
  filterSessions,
  getStatusCounts,
  groupSessionsByProject,
  type SessionStatusFilter,
} from "../state/sessionFilters";
import { useApp } from "../state/store";
import { useIsMobile } from "../hooks/useIsMobile";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { DeleteDialog } from "./DeleteDialog";

const STATUS_FILTERS: { id: SessionStatusFilter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "needs_you", label: "Needs you" },
  { id: "running", label: "Running" },
  { id: "completed", label: "Completed" },
  { id: "archived", label: "Archived" },
];

export function Sidebar() {
  const {
    sessions,
    activeId,
    openSession,
    connection,
    view,
    goHome,
    openSettings,
    openNewSession,
    toggleShortcuts,
    threads,
    sessionSearchQuery,
    sessionStatusFilter,
    pinnedSessionIds,
    archivedSessionIds,
    setSessionSearchQuery,
    setSessionStatusFilter,
    togglePinSession,
    toggleArchiveSession,
  } = useApp();
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    sessionId: string;
  } | null>(null);
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const isMobile = useIsMobile();
  const toggleSidebar = useApp((s) => s.toggleSidebar);

  const toggle = (key: string) =>
    setCollapsed((c) => {
      const next = new Set(c);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  const pinnedSet = useMemo(() => new Set(pinnedSessionIds), [pinnedSessionIds]);
  const archivedSet = useMemo(() => new Set(archivedSessionIds), [archivedSessionIds]);

  const counts = useMemo(() => getStatusCounts(sessions, archivedSet), [sessions, archivedSet]);

  const filteredSessions = useMemo(
    () =>
      filterSessions(sessions, {
        query: sessionSearchQuery,
        status: sessionStatusFilter,
        archivedIds: archivedSet,
        threads,
      }),
    [sessions, sessionSearchQuery, sessionStatusFilter, archivedSet, threads],
  );

  const groups = useMemo(
    () => groupSessionsByProject(filteredSessions, pinnedSet),
    [filteredSessions, pinnedSet],
  );

  // Close context menu on outside pointerdown or Escape
  useEffect(() => {
    if (!contextMenu) return;
    const onPointerDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setContextMenu(null);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setContextMenu(null);
    };
    window.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [contextMenu]);

  const needsYou = counts.needs_you;
  const running = counts.running;

  // On mobile, close sidebar when navigating
  const handleOpenSession = (id: string) => {
    void openSession(id);
    if (isMobile) toggleSidebar();
  };

  const handleGoHome = () => {
    goHome();
    if (isMobile) toggleSidebar();
  };

  const handleOpenSettings = () => {
    openSettings();
    if (isMobile) toggleSidebar();
  };

  const sidebar = (
    <aside
      className={cn(
        "sidebar flex flex-col gap-2 border-r border-sidebar-border bg-sidebar p-3 text-sidebar-foreground shadow-[4px_0_24px_-20px_rgba(0,0,0,0.8)]",
        isMobile
          ? "fixed inset-y-0 left-0 z-50 w-[85vw] max-w-[320px] safe-top safe-bottom"
          : "w-[288px] shrink-0",
      )}
    >
      <div className="sidebar-head flex items-center justify-between px-2 py-1">
        <Button variant="ghost" size="sm" className="flex h-auto items-center gap-2 rounded-lg px-1.5 py-1" title="Workspace menu">
          <span className="flex size-6 items-center justify-center rounded-md bg-primary text-xs font-bold text-primary-foreground">M</span>
          <span className="text-sm font-semibold tracking-tight">MauKode</span>
          <ChevronDown className="size-3.5 text-muted-foreground" />
        </Button>
        <div className="flex items-center gap-0.5">
          <Button
            variant="ghost"
            size="icon-sm"
            title="Keyboard shortcuts (?)"
            data-testid="btn-shortcuts"
            onClick={toggleShortcuts}
          >
            <CircleHelp className="size-4" />
          </Button>
          <span
            data-testid="connection"
            className={cn(
              "mx-1 size-1.5 rounded-full",
              connection === "online" && "bg-success",
              connection === "connecting" && "bg-warning",
              connection === "reconnecting" && "bg-warning",
              connection === "disconnected" && "bg-destructive",
            )}
            title={connection}
          />
        </div>
      </div>

      <div className="px-1 pt-2">
        <Button data-testid="new-session" size="sm" className="h-9 w-full justify-start gap-2 rounded-lg px-3 shadow-sm" onClick={() => openNewSession(null)}>
          <Plus className="size-4" />
          <span>New session</span>
          <kbd className="ml-auto rounded border border-primary-foreground/20 px-1.5 py-0.5 text-xs font-normal opacity-70">⌘ N</kbd>
        </Button>
      </div>

      <div className="summary flex items-center gap-2 px-2 pt-2 text-xs text-muted-foreground" data-testid="summary">
        <span className="inline-flex items-center gap-1"><span className="size-1.5 rounded-full bg-primary" />{running} running</span>
        <span className="text-border">·</span>
        <span className={cn(needsYou > 0 && "text-warning")}>{needsYou} need you</span>
      </div>

      <nav className="mt-2 space-y-0.5 px-1" aria-label="Main navigation">
        <Button
          data-testid="nav-overview"
          variant={!activeId && view === "home" ? "secondary" : "ghost"}
          size="sm"
          className="h-8 w-full justify-start gap-2 rounded-md px-2.5 text-xs"
          onClick={handleGoHome}
        >
          <House className="size-3.5" /> Overview
        </Button>
        <Button
          data-testid="nav-settings"
          variant={!activeId && view === "settings" ? "secondary" : "ghost"}
          size="sm"
          className="h-8 w-full justify-start gap-2 rounded-md px-2.5 text-xs"
          onClick={handleOpenSettings}
        >
          <Settings2 className="size-3.5" /> Settings
        </Button>
      </nav>

      <div className="my-3 h-px bg-sidebar-border" />
      <div className="flex items-center justify-between px-2">
        <span className="text-xs font-semibold uppercase tracking-[0.14em] text-muted-foreground">Sessions</span>
        <Button
          variant="ghost"
          size="icon-sm"
          title="Search sessions"
          onClick={() => document.querySelector<HTMLInputElement>("[data-testid='session-search']")?.focus()}
        >
          <Search className="size-3.5" />
        </Button>
      </div>

      <div className="sidebar-search relative">
        <Search className="pointer-events-none absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
        <Input
          data-testid="session-search"
          type="search"
          placeholder="Search sessions…"
          className="h-8 pl-8 text-xs"
          value={sessionSearchQuery}
          onChange={(e) => setSessionSearchQuery(e.target.value)}
        />
        {sessionSearchQuery && (
          <Button
            variant="ghost"
            size="icon-sm"
            className="search-clear absolute right-1 top-1/2 -translate-y-1/2"
            title="Clear search"
            aria-label="Clear search"
            onClick={() => setSessionSearchQuery("")}
          >
            <X className="size-3.5" />
          </Button>
        )}
      </div>

      {/* Status filter chips */}
      <div className="filter-chips flex flex-wrap gap-1" role="tablist" aria-label="Session filter">
        {STATUS_FILTERS.map((f) => {
          const count = counts[f.id];
          const isSelected = sessionStatusFilter === f.id;
          return (
            <Button
              key={f.id}
              variant="ghost"
              role="tab"
              data-testid={f.id === "needs_you" ? "filter-needs-you" : `filter-${f.id}`}
              aria-selected={isSelected}
              className={cn(
                "filter-chip inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs transition-colors",
                isSelected
                  ? "active border-primary/40 bg-primary/15 text-primary"
                  : "border-border text-muted-foreground hover:bg-sidebar-accent",
              )}
              onClick={() => setSessionStatusFilter(f.id)}
            >
              <span>{f.label}</span>
              {count > 0 && (
                <span
                  className={cn(
                    "chip-count rounded-full px-1 font-mono",
                    f.id === "needs_you" && "warn bg-warning/20 text-warning",
                    f.id === "running" && "run bg-primary/20 text-primary",
                  )}
                >
                  {count}
                </span>
              )}
            </Button>
          );
        })}
      </div>

      <nav className="list scrollbar-thin flex-1 space-y-2 overflow-y-auto pr-0.5">
        {groups.map(([project, items]) => {
          const closed = collapsed.has(project) && !items.some((s) => s.id === activeId);
          const attention = items.some((s) => s.pending_permission_count > 0);
          return (
            <section key={project} className="project">
              <div className="project-head group flex items-center gap-1 px-1" title={project}>
                <Button
                  variant="ghost"
                  size="sm"
                  className="project-toggle h-auto flex-1 justify-start gap-1 px-1 py-1 text-left text-xs hover:bg-sidebar-accent"
                  onClick={() => toggle(project)}
                  aria-expanded={!closed}
                >
                  <span className={cn("chev transition-transform", closed && "closed -rotate-90")}>▾</span>
                  <span className="folder text-muted-foreground" aria-hidden="true">
                    ▤
                  </span>
                  <span className="project-name truncate font-medium">{baseName(project)}</span>
                  {closed && attention && <span className="dot warn size-1.5 rounded-full bg-warning" />}
                  <span className="count ml-auto font-mono text-muted-foreground">{items.length}</span>
                </Button>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="project-add opacity-0 transition-opacity group-hover:opacity-100"
                  title={`New session in ${baseName(project)}`}
                  aria-label={`New session in ${baseName(project)}`}
                  onClick={() => openNewSession(project)}
                >
                  <Plus className="size-3.5" />
                </Button>
              </div>

              {!closed &&
                items.map((s) => {
                  const isPinned = pinnedSet.has(s.id);
                  const isArchived = archivedSet.has(s.id);
                  return (
                    <div
                      key={s.id}
                      className={cn(
                        "session-row group/row relative flex items-center rounded-md",
                        s.id === activeId ? "bg-sidebar-accent" : "hover:bg-sidebar-accent/60",
                      )}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        setContextMenu({
                          x: Math.min(e.clientX, window.innerWidth - 180),
                          y: Math.min(e.clientY, window.innerHeight - 150),
                          sessionId: s.id,
                        });
                      }}
                    >
                      <Button
                        variant="ghost"
                        data-testid="session-item"
                        className={cn(
                          "session flex min-w-0 flex-1 flex-col gap-0.5 rounded-none px-2 py-1.5 text-left text-sm",
                          s.id === activeId && "active text-sidebar-accent-foreground",
                        )}
                        onClick={() => handleOpenSession(s.id)}
                      >
                        <span className="title flex items-center gap-1 truncate">
                          {isPinned && (
                            <span className="session-pin-indicator text-primary" title="Pinned" aria-label="Pinned">
                              📌
                            </span>
                          )}
                          {s.title || "New session"}
                        </span>
                        <span className="session-meta flex items-center gap-2 text-xs text-muted-foreground">
                          {s.pending_permission_count > 0 ? (
                            <Badge variant="warning" data-testid="needs-approval" className="badge warn">
                              {s.pending_permission_count} approval
                            </Badge>
                          ) : s.running ? (
                            <Badge variant="primary" data-testid="running-badge" className="badge run">
                              running
                            </Badge>
                          ) : (
                            <span>{shortAgo(s.updated_at)}</span>
                          )}
                          {s.worktree_branch && (
                            <span className="branch truncate font-mono" title={s.worktree_branch}>
                              ⎇ {s.worktree_branch.replace(/^maukode\//, "")}
                            </span>
                          )}
                        </span>
                      </Button>

                      <div className="session-actions flex items-center gap-0.5 pr-1 opacity-0 transition-opacity group-hover/row:opacity-100">
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          data-testid="pin-session"
                          className={cn(
                            "session-action-btn",
                            isPinned && "pinned text-primary",
                          )}
                          title={isPinned ? "Unpin session" : "Pin session"}
                          aria-label={isPinned ? "Unpin session" : "Pin session"}
                          onClick={(e) => {
                            e.stopPropagation();
                            togglePinSession(s.id);
                          }}
                        >
                          <Pin className="size-3.5" fill={isPinned ? "currentColor" : "none"} />
                        </Button>
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          data-testid="archive-session"
                          className="session-action-btn"
                          title={isArchived ? "Restore session" : "Archive session"}
                          aria-label={isArchived ? "Restore session" : "Archive session"}
                          onClick={(e) => {
                            e.stopPropagation();
                            toggleArchiveSession(s.id);
                          }}
                        >
                          {isArchived ? <ArchiveRestore className="size-3.5" /> : <Archive className="size-3.5" />}
                        </Button>
                      </div>
                    </div>
                  );
                })}
            </section>
          );
        })}

        {sessions.length === 0 && <p className="px-2 py-4 text-xs text-muted-foreground">No sessions yet.</p>}
        {sessions.length > 0 && filteredSessions.length === 0 && (
          <div className="empty-filter px-2 py-4 text-xs text-muted-foreground">
            <p>
              No{" "}
              {sessionStatusFilter === "all"
                ? ""
                : sessionStatusFilter === "needs_you"
                  ? "sessions needing approval"
                  : `${sessionStatusFilter} `}
              sessions{sessionSearchQuery ? ` matching “${sessionSearchQuery}”` : ""}.
            </p>
            {(sessionSearchQuery || sessionStatusFilter !== "all") && (
              <Button
                variant="link"
                size="sm"
                className="mt-1 h-auto p-0 text-xs"
                onClick={() => {
                  setSessionSearchQuery("");
                  setSessionStatusFilter("all");
                }}
              >
                Reset filters
              </Button>
            )}
          </div>
        )}
      </nav>

      <div className="sidebar-foot mt-auto border-t border-sidebar-border pt-2">
        <Button variant="ghost" size="sm" className="flex w-full justify-start gap-2 rounded-lg px-2 py-2 text-xs" onClick={toggleShortcuts}>
          <CircleHelp className="size-3.5" /> Help & shortcuts <kbd className="ml-auto text-xs opacity-60">?</kbd>
        </Button>
      </div>

      {contextMenu && (
        <div
          className="session-context-menu fixed z-50 min-w-[180px] animate-in fade-in-0 zoom-in-95 overflow-hidden rounded-lg border border-border bg-popover p-1 text-sm text-popover-foreground shadow-xl"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          role="menu"
        >
          <Button
            variant="ghost"
            role="menuitem"
            data-testid="context-menu-pin"
            className="context-menu-item flex w-full justify-start gap-2 rounded-md px-2 py-1.5 text-left text-sm"
            onClick={() => {
              togglePinSession(contextMenu.sessionId);
              setContextMenu(null);
            }}
          >
            <Pin className="size-4 shrink-0" fill={pinnedSet.has(contextMenu.sessionId) ? "currentColor" : "none"} />
            <span>{pinnedSet.has(contextMenu.sessionId) ? "Unpin session" : "Pin to top"}</span>
          </Button>
          <Button
            variant="ghost"
            role="menuitem"
            data-testid="context-menu-archive"
            className="context-menu-item flex w-full justify-start gap-2 rounded-md px-2 py-1.5 text-left text-sm"
            onClick={() => {
              toggleArchiveSession(contextMenu.sessionId);
              setContextMenu(null);
            }}
          >
            {archivedSet.has(contextMenu.sessionId) ? (
              <ArchiveRestore className="size-4 shrink-0" />
            ) : (
              <Archive className="size-4 shrink-0" />
            )}
            <span>{archivedSet.has(contextMenu.sessionId) ? "Restore session" : "Archive session"}</span>
          </Button>
          <div className="context-divider -mx-1 my-1 h-px bg-border" />
          <Button
            variant="ghost"
            role="menuitem"
            data-testid="context-menu-delete"
            className="context-menu-item danger flex w-full justify-start gap-2 rounded-md px-2 py-1.5 text-left text-sm text-destructive hover:bg-destructive/10"
            onClick={() => {
              const id = contextMenu.sessionId;
              setContextMenu(null);
              setDeletingId(id);
            }}
          >
            <Trash2 className="size-4 shrink-0" />
            <span>Delete session…</span>
          </Button>
        </div>
      )}

      {deletingId && <DeleteDialog sessionId={deletingId} onClose={() => setDeletingId(null)} />}
    </aside>
  );

  // On mobile: overlay with backdrop; on desktop: inline sidebar
  if (isMobile) {
    return (
      <>
        {/* Backdrop */}
        <div
          className="fixed inset-0 z-40 bg-black/50 backdrop-blur-sm"
          onClick={toggleSidebar}
        />
        {sidebar}
      </>
    );
  }

  return sidebar;
}
