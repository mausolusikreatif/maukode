import { useEffect, useMemo, useRef, useState } from "react";
import { baseName } from "../state/format";
import { useApp } from "../state/store";

interface PaletteItem {
  id: string;
  category: "Actions" | "Sessions";
  title: string;
  description?: string;
  shortcut?: string;
  badge?: string;
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
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const currentThread = activeId ? threads[activeId] : null;
  const isRunning = Boolean(currentThread?.running);

  const actions = useMemo<PaletteItem[]>(() => {
    return [
      {
        id: "new-task",
        category: "Actions",
        title: "New Task",
        description: "Create a new agent task or session",
        shortcut: "Ctrl+N",
        run: () => {
          closeCommandPalette();
          openNewSession();
        },
      },
      {
        id: "jump-needs-you",
        category: "Actions",
        title: "Jump to Needs You",
        description: "Switch to the first session waiting for approval",
        run: () => {
          const needy = sessions.find((s) => s.pending_permission_count > 0);
          if (needy) {
            void openSession(needy.id);
          } else {
            goHome();
          }
          closeCommandPalette();
        },
      },
      {
        id: "toggle-sidebar",
        category: "Actions",
        title: "Toggle Sidebar",
        description: "Show or hide the sessions sidebar",
        shortcut: "Ctrl+B",
        run: () => {
          toggleSidebar();
          closeCommandPalette();
        },
      },
      {
        id: "toggle-review-diff",
        category: "Actions",
        title: "Toggle Review Diff",
        description: "Open or close the changed files review panel",
        shortcut: "Ctrl+Shift+D",
        run: () => {
          if (activeId) {
            void toggleReview();
          }
          closeCommandPalette();
        },
      },
      {
        id: "focus-composer",
        category: "Actions",
        title: "Focus Composer",
        description: "Focus the message input box",
        shortcut: "Ctrl+L",
        run: () => {
          closeCommandPalette();
          setTimeout(() => {
            const el = document.querySelector<HTMLTextAreaElement>(
              'textarea[data-testid="composer-input"]'
            );
            el?.focus();
          }, 50);
        },
      },
      {
        id: "stop-current-task",
        category: "Actions",
        title: "Stop Current Task",
        description: isRunning
          ? "Interrupt the running agent turn"
          : "No task is currently running",
        shortcut: "Ctrl+Esc",
        run: () => {
          if (isRunning) {
            void stop();
          }
          closeCommandPalette();
        },
      },
      {
        id: "shortcuts",
        category: "Actions",
        title: "Keyboard Shortcuts",
        description: "Show cheat sheet of keyboard shortcuts",
        shortcut: "?",
        run: () => {
          closeCommandPalette();
          toggleShortcuts();
        },
      },
      {
        id: "go-home",
        category: "Actions",
        title: "Go to Overview",
        description: "Switch to dashboard overview",
        run: () => {
          goHome();
          closeCommandPalette();
        },
      },
      {
        id: "open-settings",
        category: "Actions",
        title: "Open Settings",
        description: "Configure providers, approval modes, and limits",
        run: () => {
          openSettings();
          closeCommandPalette();
        },
      },
    ];
  }, [
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
  ]);

  const sessionItems = useMemo<PaletteItem[]>(() => {
    return sessions.map((s) => ({
      id: `session-${s.id}`,
      category: "Sessions",
      title: s.title || "Untitled task",
      description: `${baseName(s.project_dir ?? s.directory)}${
        s.worktree_branch ? ` · ⎇ ${s.worktree_branch}` : ""
      }`,
      badge:
        s.pending_permission_count > 0
          ? `${s.pending_permission_count} approval`
          : s.running
          ? "running"
          : undefined,
      run: () => {
        void openSession(s.id);
        closeCommandPalette();
      },
    }));
  }, [sessions, openSession, closeCommandPalette]);

  const filtered = useMemo<PaletteItem[]>(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      // All quick actions plus up to 8 sessions
      return [...actions, ...sessionItems.slice(0, 8)];
    }
    const matchingActions = actions.filter(
      (a) =>
        a.title.toLowerCase().includes(q) ||
        (a.description && a.description.toLowerCase().includes(q))
    );
    const matchingSessions = sessionItems.filter(
      (s) =>
        s.title.toLowerCase().includes(q) ||
        (s.description && s.description.toLowerCase().includes(q))
    );
    return [...matchingActions, ...matchingSessions];
  }, [actions, sessionItems, query]);

  // Reset selected index when query results change
  useEffect(() => {
    setSelectedIndex(0);
  }, [query]);

  // Scroll selected item into view
  useEffect(() => {
    if (!listRef.current) return;
    const selectedEl = listRef.current.children[selectedIndex] as HTMLElement | undefined;
    selectedEl?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "ArrowDown" || e.key === "Down") {
      e.preventDefault();
      setSelectedIndex((i) => (filtered.length > 0 ? (i + 1) % filtered.length : 0));
    } else if (e.key === "ArrowUp" || e.key === "Up") {
      e.preventDefault();
      setSelectedIndex((i) =>
        filtered.length > 0 ? (i - 1 + filtered.length) % filtered.length : 0
      );
    } else if (e.key === "Enter") {
      e.preventDefault();
      const current = filtered[selectedIndex];
      if (current) {
        current.run();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      closeCommandPalette();
    }
  };

  return (
    <div
      className="palette-backdrop"
      data-testid="palette-backdrop"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          closeCommandPalette();
        }
      }}
    >
      <div
        className="palette-dialog"
        data-testid="command-palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command Palette"
      >
        <div className="palette-header">
          <span className="palette-search-icon" aria-hidden>
            🔍
          </span>
          <input
            ref={inputRef}
            autoFocus
            data-testid="palette-input"
            className="palette-input"
            placeholder="Type a command or search sessions…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={handleKeyDown}
          />
        </div>

        <div className="palette-list" data-testid="palette-list" ref={listRef}>
          {filtered.map((item, index) => {
            const isSelected = index === selectedIndex;
            return (
              <button
                key={item.id}
                type="button"
                data-testid="palette-item"
                className={`palette-item ${isSelected ? "active" : ""}`}
                onClick={item.run}
                onMouseEnter={() => setSelectedIndex(index)}
              >
                <div className="palette-item-left">
                  <span className="palette-item-title">{item.title}</span>
                  {item.description && (
                    <span className="palette-item-desc">{item.description}</span>
                  )}
                </div>
                <div className="palette-item-right">
                  {item.badge && (
                    <span className="badge warn palette-badge">{item.badge}</span>
                  )}
                  {item.shortcut && (
                    <kbd className="palette-shortcut">{item.shortcut}</kbd>
                  )}
                </div>
              </button>
            );
          })}

          {filtered.length === 0 && (
            <div className="palette-empty" data-testid="palette-empty">
              No matching commands or sessions.
            </div>
          )}
        </div>

        <div className="palette-footer">
          <span>
            <kbd>↑</kbd> <kbd>↓</kbd> to navigate
          </span>
          <span>
            <kbd>Enter</kbd> to select
          </span>
          <span>
            <kbd>Esc</kbd> to close
          </span>
        </div>
      </div>
    </div>
  );
}
