import { useEffect, useMemo, useRef, useState } from "react";
import { Code2, FileText, Folder, GitBranch, Plus } from "lucide-react";
import type { PermissionMode } from "../api/types";
import { useApp } from "../state/store";
import { cn } from "@/lib/utils";
import { ModelSelect } from "./ModelSelect";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "./ui/dropdown-menu";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./ui/select";
import {
  buildAutocompleteItems,
  chipIcon,
  estimateTokens,
  parseRepoMap,
  type AutocompleteItem,
  type ContextChip,
  type ContextKind,
  type ParsedCodebase,
} from "./composerContext";

const PERMISSION_MODES: { value: PermissionMode; label: string }[] = [
  { value: "ask", label: "Ask before risky actions" },
  { value: "strict", label: "Ask for everything" },
  { value: "review", label: "Review (private results)" },
  { value: "trust", label: "Auto-approve (trust)" },
];

function ModeSelect({ sessionId }: { sessionId: string }) {
  const client = useApp((s) => s.client);
  const [mode, setMode] = useState<PermissionMode | "">("");

  useEffect(() => {
    let cancelled = false;
    setMode("");
    if (sessionId && client) {
      void client
        .request<{ mode: PermissionMode }>("GET", `/api/v1/permissions/mode?session_id=${sessionId}`)
        .then((r) => !cancelled && setMode(r.mode))
        .catch(() => undefined);
    }
    return () => {
      cancelled = true;
    };
  }, [client, sessionId]);

  if (!mode) {
    return (
      <button
        type="button"
        disabled
        data-testid="perm-mode"
        className="composer-select perm-mode-select inline-flex h-8 w-auto items-center gap-1 rounded-md border border-border bg-card/60 px-2.5 py-1 text-xs opacity-50"
      >
        <span className="text-muted-foreground">Mode…</span>
      </button>
    );
  }

  return (
    <Select
      value={mode}
      onValueChange={(next) => {
        const m = next as PermissionMode;
        setMode(m);
        if (client && sessionId) {
          void client.setPermissionMode(m, sessionId);
        }
      }}
    >
      <SelectTrigger
        data-testid="perm-mode"
        className="composer-select perm-mode-select h-8 w-auto gap-1 rounded-md border border-border bg-card/60 px-2.5 py-1 text-xs"
        title="Session permission mode"
      >
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {PERMISSION_MODES.map((m) => (
          <SelectItem key={m.value} value={m.value}>
            {m.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}

export function Composer({ running }: { running: boolean }) {
  const [text, setText] = useState("");
  const [chips, setChips] = useState<ContextChip[]>([]);
  const [autocompleteOpen, setAutocompleteOpen] = useState(false);
  const [autocompleteQuery, setAutocompleteQuery] = useState("");
  const [autocompleteMode, setAutocompleteMode] = useState<"all" | "file" | "folder" | "symbol">("all");
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const [codebase, setCodebase] = useState<ParsedCodebase>({ files: [], folders: [], symbols: [] });

  const composerRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const autocompleteListRef = useRef<HTMLDivElement>(null);

  const { send, stop, steer, queue, activeId, meta, review, client, threads } = useApp();
  const thread = activeId ? threads[activeId] : undefined;

  // Auto-resize textarea
  useEffect(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const next = Math.min(el.scrollHeight, 220);
    el.style.height = `${Math.max(32, next)}px`;
  }, [text]);

  // Load workspace repo map when active session changes
  useEffect(() => {
    if (!client || !activeId) return;
    let cancelled = false;

    void client
      .codebaseMap(activeId, 1000)
      .then((res) => {
        if (!cancelled && res?.map) {
          const parsed = parseRepoMap(res.map);
          setCodebase(parsed);
        }
      })
      .catch(() => {
        // Fallback gracefully if map is not supported by environment
      });

    return () => {
      cancelled = true;
    };
  }, [client, activeId]);

  // Close autocomplete when clicking outside the composer
  useEffect(() => {
    const handleClickOutside = (e: MouseEvent) => {
      if (composerRef.current && !composerRef.current.contains(e.target as Node)) {
        setAutocompleteOpen(false);
      }
    };
    document.addEventListener("mousedown", handleClickOutside);
    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
    };
  }, []);

  // Compute filtered autocomplete suggestions
  const filteredItems = useMemo(() => {
    return buildAutocompleteItems({
      query: autocompleteQuery,
      mode: autocompleteMode,
      codebase,
      changedFiles: review.files.map((f) => ({ path: f.path, status: f.status })),
      diffCount: review.files.length,
    });
  }, [autocompleteQuery, autocompleteMode, codebase, review.files]);

  // Reset highlight when items change
  useEffect(() => {
    setHighlightedIndex(0);
  }, [filteredItems.length]);

  // Scroll highlighted item into view
  useEffect(() => {
    if (autocompleteOpen && autocompleteListRef.current) {
      const el = autocompleteListRef.current.querySelector<HTMLElement>(`[data-index="${highlightedIndex}"]`);
      el?.scrollIntoView({ block: "nearest" });
    }
  }, [highlightedIndex, autocompleteOpen]);

  // Check for `@` mention trigger in text
  const checkMentionTrigger = (val: string, cursor: number) => {
    const beforeCursor = val.slice(0, cursor);
    const match = /(?:^|\s)@([a-zA-Z0-9_\-\.\/]*)$/.exec(beforeCursor);
    if (match) {
      setAutocompleteQuery(match[1]);
      setAutocompleteMode("all");
      setAutocompleteOpen(true);
    } else {
      setAutocompleteOpen(false);
    }
  };

  const handleTextChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    setText(val);
    checkMentionTrigger(val, e.target.selectionStart ?? val.length);
  };

  const addChip = (chip: ContextChip) => {
    setChips((prev) => (prev.some((c) => c.value === chip.value) ? prev : [...prev, chip]));
  };

  const removeChip = (id: string) => {
    setChips((prev) => prev.filter((c) => c.id !== id));
  };

  const selectItem = (item: AutocompleteItem) => {
    if (item.kind === "action") {
      // Switched into specialized category mode (e.g. browsing files or symbols)
      if (item.value === "file") setAutocompleteMode("file");
      else if (item.value === "folder") setAutocompleteMode("folder");
      else if (item.value === "symbol") setAutocompleteMode("symbol");
      setAutocompleteQuery("");
      textareaRef.current?.focus();
      return;
    }

    // Specific reference: attach chip and replace typed mention
    const kind = item.kind as ContextKind;
    addChip({
      id: `${kind}-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
      kind,
      value: item.value,
      label: `@${item.value}`,
      detail: item.description,
    });

    // Remove the `@query` from text
    const cursor = textareaRef.current?.selectionStart ?? text.length;
    const beforeCursor = text.slice(0, cursor);
    const match = /(?:^|\s)@([a-zA-Z0-9_\-\.\/]*)$/.exec(beforeCursor);
    if (match) {
      const startIdx = match.index + (match[0].startsWith("@") ? 0 : 1);
      const before = text.slice(0, startIdx);
      const after = text.slice(cursor);
      const newText = (before + after).replace(/[ \t]+$/, "");
      setText(newText);
    }

    setAutocompleteOpen(false);
    setAutocompleteMode("all");
    setAutocompleteQuery("");
    textareaRef.current?.focus();
  };

  const openPickerMode = (mode: "file" | "folder" | "symbol") => {
    setAutocompleteMode(mode);
    setAutocompleteQuery("");
    setAutocompleteOpen(true);
    textareaRef.current?.focus();
  };

  const clearInput = () => {
    setText("");
    setChips([]);
    setAutocompleteOpen(false);
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  };

  const buildPrompt = () => {
    const value = text.trim();
    if (!value && chips.length === 0) return "";
    const contextTags = chips.map((c) => `@${c.value}`).join(" ");
    if (contextTags) {
      return value ? `${contextTags}\n\n${value}` : contextTags;
    }
    return value;
  };

  const handleSend = () => {
    const prompt = buildPrompt();
    if (!prompt || running) return;
    clearInput();
    void send(prompt);
  };

  const handleSteer = () => {
    const prompt = buildPrompt();
    if (!prompt) return;
    clearInput();
    void steer(prompt);
  };

  const handleQueue = () => {
    const prompt = buildPrompt();
    if (!prompt) return;
    clearInput();
    void queue(prompt);
  };

  const submit = handleSend;





  const gauge = estimateTokens(text, chips);

  const gaugeTooltip = `Estimated prompt: ~${gauge.tokens} tokens (${gauge.chars} chars)${
    thread?.usage
      ? ` · Session tokens: ${thread.usage.input_tokens.toLocaleString()} input, ${thread.usage.output_tokens.toLocaleString()} output`
      : ""
  }`;

  return (
    <div
      ref={composerRef}
      className={cn(
        "composer relative border-t border-border/70 bg-card/40 px-4 py-3",
        running && "running",
      )}
    >
      {/* Attached Context Chips */}
      {chips.length > 0 && (
        <div className="composer-chips mb-2 flex flex-wrap items-center gap-1.5" data-testid="composer-chips">
          {chips.map((chip) => (
            <span
              key={chip.id}
              className={cn(
                "context-chip inline-flex items-center gap-1 rounded-full border border-border bg-secondary px-2 py-0.5 text-xs",
                `context-chip-${chip.kind}`,
              )}
              data-testid="context-chip"
            >
              <span className="chip-icon text-tool">{chipIcon(chip.kind)}</span>
              <span className="chip-label max-w-[16rem] truncate" title={chip.value}>
                {chip.label}
              </span>
              {chip.detail && <span className="chip-detail text-muted-foreground">{chip.detail}</span>}
              <button
                type="button"
                className="chip-dismiss ml-0.5 rounded-full px-1 text-muted-foreground hover:bg-accent hover:text-foreground"
                data-testid="chip-dismiss"
                aria-label={`Remove ${chip.label}`}
                onClick={(e) => {
                  e.stopPropagation();
                  removeChip(chip.id);
                }}
                title="Remove attachment"
              >
                ×
              </button>
            </span>
          ))}
          {chips.length > 1 && (
            <button
              type="button"
              className="chip-clear text-xs text-primary hover:underline"
              onClick={() => setChips([])}
              title="Clear all attachments"
            >
              Clear all
            </button>
          )}
        </div>
      )}

      {/* Autocomplete Popup */}
      {autocompleteOpen && (
        <div
          ref={autocompleteListRef}
          className="composer-autocomplete absolute bottom-full left-4 z-40 mb-2 w-[min(36rem,80vw)] animate-in fade-in-0 zoom-in-95 overflow-hidden rounded-lg border border-border bg-popover shadow-xl"
          data-testid="composer-autocomplete"
        >
          <div className="autocomplete-header flex items-center justify-between border-b border-border px-3 py-1.5 text-xs">
            <span className="font-medium">
              {autocompleteMode === "all"
                ? "Context Suggestions"
                : autocompleteMode === "file"
                  ? "Select Workspace File"
                  : autocompleteMode === "folder"
                    ? "Select Folder"
                    : "Select Symbol"}
            </span>
            <span className="text-muted-foreground">↑↓ navigate · ↵ select · esc dismiss</span>
          </div>
          {filteredItems.length === 0 ? (
            <div className="autocomplete-empty px-3 py-3 text-sm text-muted-foreground">
              No matching context references found
            </div>
          ) : (
            filteredItems.map((item, idx) => (
              <button
                key={item.id}
                type="button"
                data-index={idx}
                className={cn(
                  "autocomplete-item flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-accent",
                  idx === highlightedIndex && "bg-accent",
                )}
                onMouseEnter={() => setHighlightedIndex(idx)}
                onClick={() => selectItem(item)}
              >
                <span className="item-icon w-5 text-center text-tool">{item.icon}</span>
                <div className="item-main flex min-w-0 flex-col">
                  <span className="item-label truncate font-mono text-xs">{item.label}</span>
                  <span className="item-desc truncate text-xs text-muted-foreground">{item.description}</span>
                </div>
              </button>
            ))
          )}
        </div>
      )}

      {/* Textarea Input */}
      <div className="composer-input-row">
        <textarea
          ref={textareaRef}
          data-testid="composer-input"
          value={text}
          placeholder={
            running
              ? "Agent is working… Press Stop to interrupt"
              : "Message the agent… Type @ for context, Enter to send, Shift+Enter for new line"
          }
          onChange={handleTextChange}
          onKeyDown={(e) => {
            if (autocompleteOpen) {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setHighlightedIndex((i) => (filteredItems.length > 0 ? (i + 1) % filteredItems.length : 0));
                return;
              }
              if (e.key === "ArrowUp") {
                e.preventDefault();
                setHighlightedIndex((i) =>
                  filteredItems.length > 0 ? (i - 1 + filteredItems.length) % filteredItems.length : 0,
                );
                return;
              }
              if (e.key === "Enter" || e.key === "Tab") {
                if (filteredItems.length > 0 && highlightedIndex >= 0 && highlightedIndex < filteredItems.length) {
                  e.preventDefault();
                  selectItem(filteredItems[highlightedIndex]);
                  return;
                }
              }
              if (e.key === "Escape") {
                e.preventDefault();
                setAutocompleteOpen(false);
                return;
              }
            }

            if (e.key === "Backspace" && text === "" && chips.length > 0) {
              setChips((prev) => prev.slice(0, prev.length - 1));
              return;
            }

            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              if (running) {
                if (text.trim() || chips.length > 0) handleQueue();
              } else {
                submit();
              }
            } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              handleSteer();
            }
          }}
          rows={3}
          className="w-full resize-none rounded-md border border-input bg-background/60 px-3 py-2 text-sm leading-relaxed outline-none placeholder:text-muted-foreground focus:ring-2 focus:ring-ring scrollbar-thin"
        />
      </div>

      {/* Composer Footer Toolbar */}
      <div className="composer-footer mt-2 flex items-center justify-between gap-3">
        <div className="composer-footer-left flex items-center gap-2">
          {/* Attach Button & Dropdown */}
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className="attach-btn inline-flex items-center gap-1 rounded-md border border-border px-2.5 py-1 text-xs hover:bg-accent"
                data-testid="attach-button"
                title="Add context reference (@file, @folder, @git-diff, @symbol)"
              >
                <Plus className="size-3" />
                Attach
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="start"
              side="top"
              sideOffset={4}
              className="w-48"
            >
              <DropdownMenuItem
                data-testid="attach-dropdown"
                onClick={() => openPickerMode("file")}
              >
                <FileText className="size-4" />
                Attach File…
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => openPickerMode("folder")}>
                <Folder className="size-4" />
                Attach Folder…
              </DropdownMenuItem>
              <DropdownMenuItem
                onClick={() => {
                  addChip({
                    id: `diff-${Date.now()}`,
                    kind: "diff",
                    value: "git-diff",
                    label: "@git-diff",
                    detail:
                      review.files.length > 0 ? `${review.files.length} changed files` : undefined,
                  });
                }}
              >
                <GitBranch className="size-4" />
                Current Git Diff
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => openPickerMode("symbol")}>
                <Code2 className="size-4" />
                Code Symbol…
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>

          {/* Environment / Branch Capability Indicator */}
          <span
            className="env-indicator inline-flex items-center gap-1.5 rounded-md px-1.5 py-1 text-xs text-muted-foreground"
            title={`Workspace: ${meta?.projectDir ?? meta?.directory ?? "Local"}${
              meta?.branch ? ` (branch: ${meta.branch})` : ""
            }`}
          >
            <span className="env-dot size-1.5 rounded-full bg-success" />
            <GitBranch className="size-3" />
            <span className="env-label font-mono">{meta?.branch ?? "Local"}</span>
          </span>

          {/* Model Selector in Composer Footer */}
          <ModelSelect
            sessionId={activeId ?? ""}
            currentProvider={meta?.provider ?? null}
            currentModel={meta?.model ?? null}
            className="composer-model-select"
          />

          {/* Mode / Permission Level Selector in Composer Footer */}
          <ModeSelect sessionId={activeId ?? ""} />
        </div>

        <div className="composer-footer-right flex items-center gap-2">
          {/* Token / Context Gauge */}
          <div
            className="composer-gauge inline-flex items-center gap-1 rounded-md bg-secondary px-2 py-1 text-xs text-muted-foreground"
            data-testid="token-gauge"
            title={gaugeTooltip}
          >
            <span className="gauge-icon">⚡</span>
            <span className="gauge-tokens font-mono">~{gauge.tokens} tok</span>
          </div>

          {/* Send / Stop / Steer / Queue Buttons */}
          {running ? (
            <div className="composer-actions flex items-center gap-2">
              <button
                type="button"
                data-testid="steer-button"
                className="steer-btn rounded-md border border-tool/50 px-3 py-1.5 text-xs font-medium text-tool hover:bg-tool/10"
                disabled={!text.trim() && chips.length === 0}
                onClick={handleSteer}
                title="Steer agent immediately with new guidance (Cmd+Enter)"
              >
                Steer
              </button>
              <button
                type="button"
                data-testid="queue-button"
                className="queue-btn rounded-md border border-border px-3 py-1.5 text-xs font-medium hover:bg-accent"
                disabled={!text.trim() && chips.length === 0}
                onClick={handleQueue}
                title="Queue message for next turn (Enter)"
              >
                Queue
              </button>
              <button
                type="button"
                data-testid="stop-button"
                className="danger rounded-md bg-destructive px-3 py-1.5 text-xs font-medium text-destructive-foreground hover:bg-destructive/90"
                onClick={() => void stop()}
                title="Interrupt the agent turn"
              >
                Stop
              </button>
            </div>
          ) : (
            <button
              data-testid="send-button"
              className="primary rounded-md bg-primary px-4 py-1.5 text-xs font-semibold text-primary-foreground hover:bg-primary/90 disabled:opacity-50"
              disabled={!text.trim() && chips.length === 0}
              onClick={submit}
            >
              Send
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
