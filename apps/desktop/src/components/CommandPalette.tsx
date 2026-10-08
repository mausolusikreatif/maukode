import { useMemo, useState, type ReactNode } from "react";
import { GitBranch, Play, Plus, Search, Settings, Square, PanelLeft, Eye, Keyboard, Home, Zap } from "lucide-react";
import { baseName } from "../state/format";
import { Badge } from "./ui/badge";
import { CommandDialog, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "./ui/command";
import { useApp } from "../state/store";
import { primaryMod } from "../platform";

interface PaletteItem {
  id: string;
  title: string;
  description?: string;
  shortcut?: string;
  badge?: string;
  icon: ReactNode;
  run: () => void;
}

export function CommandPalette() {
  const {
    sessions,
    activeId,
    threads,
    openSession,
    toggleReview,
    stop,
    goHome,
    openSettings,
    toggleSidebar,
    openNewSession,
    toggleShortcuts,
    closeCommandPalette,
  } = useApp();

  const [query, setQuery] = useState("");
  const mod = primaryMod();
  const currentThread = activeId ? threads[activeId] : null;
  const isRunning = Boolean(currentThread?.running);

  const actions = useMemo<PaletteItem[]>(
    () => [
      {
        id: "new-task",
        title: "New Task",
        description: "Create a new agent task or session",
        shortcut: `${mod}+N`,
        icon: <Plus className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          closeCommandPalette();
          openNewSession();
        },
      },
      {
        id: "jump-needs-you",
        title: "Jump to Needs You",
        description: "Switch to the first session waiting for approval",
        icon: <Zap className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          const needy = sessions.find((s) => s.pending_permission_count > 0);
          if (needy) void openSession(needy.id);
          else goHome();
          closeCommandPalette();
        },
      },
      {
        id: "toggle-sidebar",
        title: "Toggle Sidebar",
        description: "Show or hide the sessions sidebar",
        shortcut: `${mod}+B`,
        icon: <PanelLeft className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          toggleSidebar();
          closeCommandPalette();
        },
      },
      {
        id: "toggle-review-diff",
        title: "Toggle Review Diff",
        description: "Open or close the changed files review panel",
        shortcut: `${mod}+Shift+D`,
        icon: <Eye className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          if (activeId) void toggleReview();
          closeCommandPalette();
        },
      },
      {
        id: "focus-composer",
        title: "Focus Composer",
        description: "Focus the message input box",
        shortcut: `${mod}+L`,
        icon: <Keyboard className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          closeCommandPalette();
          setTimeout(() => {
            document.querySelector<HTMLTextAreaElement>('textarea[data-testid="composer-input"]')?.focus();
          }, 50);
        },
      },
      {
        id: "stop-current-task",
        title: "Stop Current Task",
        description: isRunning ? "Interrupt the running agent turn" : "No task is currently running",
        shortcut: `${mod}+Esc`,
        icon: <Square className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          if (isRunning) void stop();
          closeCommandPalette();
        },
      },
      {
        id: "shortcuts",
        title: "Keyboard Shortcuts",
        description: "Show cheat sheet of keyboard shortcuts",
        shortcut: "?",
        icon: <Search className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          closeCommandPalette();
          toggleShortcuts();
        },
      },
      {
        id: "go-home",
        title: "Go to Overview",
        description: "Switch to dashboard overview",
        icon: <Home className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          goHome();
          closeCommandPalette();
        },
      },
      {
        id: "open-settings",
        title: "Open Settings",
        description: "Configure providers, approval modes, and limits",
        icon: <Settings className="size-4 shrink-0 text-muted-foreground" />,
        run: () => {
          openSettings();
          closeCommandPalette();
        },
      },
    ],
    [
      mod,
      sessions,
      activeId,
      isRunning,
      closeCommandPalette,
      openNewSession,
      openSession,
      goHome,
      toggleSidebar,
      toggleReview,
      stop,
      toggleShortcuts,
      openSettings,
    ],
  );

  const sessionItems = useMemo<PaletteItem[]>(
    () =>
      sessions.map((s) => ({
        id: `session-${s.id}`,
        title: s.title || "Untitled task",
        description: `${baseName(s.project_dir ?? s.directory)}${s.worktree_branch ? ` · ${s.worktree_branch}` : ""}`,
        badge:
          s.pending_permission_count > 0
            ? `${s.pending_permission_count} approval`
            : s.running
              ? "running"
              : undefined,
        icon: s.running ? (
          <Play className="size-4 shrink-0 text-primary" />
        ) : (
          <GitBranch className="size-4 shrink-0 text-muted-foreground" />
        ),
        run: () => {
          void openSession(s.id);
          closeCommandPalette();
        },
      })),
    [sessions, openSession, closeCommandPalette],
  );

  const visibleSessions = query.trim() ? sessionItems : sessionItems.slice(0, 8);
  const filter = (value: string, search: string) =>
    value.toLowerCase().includes(search.trim().toLowerCase()) ? 1 : 0;

  return (
    <CommandDialog
      open
      onOpenChange={(open) => !open && closeCommandPalette()}
      className="max-w-xl"
      contentTestId="command-palette"
      filter={filter}
    >
      <CommandInput
        autoFocus
        data-testid="palette-input"
        placeholder="Type a command or search sessions…"
        value={query}
        onValueChange={setQuery}
      />
      <CommandList data-testid="palette-list">
        <CommandEmpty data-testid="palette-empty">No matching commands or sessions.</CommandEmpty>
        <CommandGroup heading="Actions">
          {actions.map((item) => (
            <PaletteRow key={item.id} item={item} />
          ))}
        </CommandGroup>
        {visibleSessions.length > 0 && (
          <CommandGroup heading="Sessions">
            {visibleSessions.map((item) => (
              <PaletteRow key={item.id} item={item} />
            ))}
          </CommandGroup>
        )}
      </CommandList>
      <div className="flex items-center gap-4 border-t border-border px-3 py-2 text-[11px] text-muted-foreground">
        <span>
          <kbd className="rounded border border-border bg-secondary px-1">↑</kbd>{" "}
          <kbd className="rounded border border-border bg-secondary px-1">↓</kbd> to navigate
        </span>
        <span>
          <kbd className="rounded border border-border bg-secondary px-1">Enter</kbd> to select
        </span>
        <span>
          <kbd className="rounded border border-border bg-secondary px-1">Esc</kbd> to close
        </span>
      </div>
    </CommandDialog>
  );
}

function PaletteRow({ item }: { item: PaletteItem }) {
  return (
    <CommandItem
      value={`${item.title} ${item.description ?? ""}`}
      data-testid="palette-item"
      onSelect={item.run}
      className="justify-between"
    >
      <span className="flex min-w-0 items-center gap-2">
        {item.icon}
        <span className="flex min-w-0 flex-col">
          <span className="truncate text-sm">{item.title}</span>
          {item.description && (
            <span className="truncate text-xs text-muted-foreground">{item.description}</span>
          )}
        </span>
      </span>
      <span className="flex shrink-0 items-center gap-2">
        {item.badge && <Badge variant="warning">{item.badge}</Badge>}
        {item.shortcut && (
          <kbd className="rounded border border-border bg-secondary px-1.5 py-0.5 font-mono text-[11px] text-muted-foreground">
            {item.shortcut}
          </kbd>
        )}
      </span>
    </CommandItem>
  );
}
