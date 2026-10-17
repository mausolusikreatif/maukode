import { useEffect, useLayoutEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { X } from "lucide-react";
import { useApp } from "../state/store";
import type { SubagentDetail, ThreadItem, ThreadState } from "../state/thread";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { PlanTimeline } from "./PlanTimeline";

function prettyArgs(args: string): string {
  try {
    return JSON.stringify(JSON.parse(args), null, 2);
  } catch {
    return args;
  }
}

/** One-line summary of what a tool call acts on (path, command, pattern…). */
function argHint(args: string): string {
  try {
    const o = JSON.parse(args) as Record<string, unknown>;
    for (const k of ["path", "file_path", "command", "pattern", "query", "url", "directory"]) {
      if (typeof o[k] === "string") return (o[k] as string).split("\n")[0];
    }
  } catch {
    /* partial JSON while streaming */
  }
  return "";
}

function CodeBlock({ className, children, ...props }: React.ComponentProps<"code">) {
  const [copied, setCopied] = useState(false);
  const match = /language-(\w+)/.exec(className || "");
  const isInline = !match && typeof children === "string" && !children.includes("\n");

  if (isInline) {
    return (
      <code className="rounded bg-secondary px-1 py-0.5 font-mono text-[0.85em]" {...props}>
        {children}
      </code>
    );
  }

  const codeText = String(children).replace(/\n$/, "");

  const copy = () => {
    void navigator.clipboard.writeText(codeText);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="code-block my-2 overflow-hidden rounded-lg border border-border bg-background/70">
      <div className="code-head flex items-center justify-between border-b border-border px-3 py-1 text-xs text-muted-foreground">
        <span className="code-lang font-mono">{match ? match[1] : "text"}</span>
        <Button variant="ghost" size="xs" className="code-copy-btn touch-target" onClick={copy} title="Copy code">
          {copied ? "Copied!" : "Copy"}
        </Button>
      </div>
      <pre className="scrollbar-thin overflow-x-auto p-3 font-mono text-xs leading-relaxed">
        <code className={className} {...props}>
          {children}
        </code>
      </pre>
    </div>
  );
}

function ToolCard({ item }: { item: Extract<ThreadItem, { kind: "tool" }> }) {
  const [copied, setCopied] = useState(false);
  const label = { running: "running…", done: "done", error: "failed", private: "shown only to you" }[item.status];

  const copyOutput = (e: React.MouseEvent) => {
    e.stopPropagation();
    const content = item.result ?? item.args ?? "";
    void navigator.clipboard.writeText(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <details
      data-testid="tool-card"
      className={cn("tool group/tool rounded-lg border border-border bg-card/50", `tool-${item.status}`)}
      open={item.status === "running" ? true : undefined}
    >
      <summary className="flex cursor-pointer list-none items-center gap-2 px-3 py-2 text-sm [&::-webkit-details-marker]:hidden">
        <span className="tool-name font-mono font-medium text-tool">{item.name || "tool"}</span>
        <span className="tool-hint truncate font-mono text-xs text-muted-foreground">
          {argHint(item.args ?? "")}
        </span>
        <span data-testid="tool-status" className="tool-status ml-auto text-xs text-muted-foreground">
          {label}
        </span>
      </summary>
      <div className="tool-body space-y-2 border-t border-border px-3 py-2">
        {item.args && (
          <div className="tool-section">
            <span className="tool-sec-label sidebar-section-label">Input</span>
            <pre className="args scrollbar-thin mt-1 overflow-x-auto rounded-md bg-background/70 p-2 font-mono text-xs">
              {prettyArgs(item.args)}
            </pre>
          </div>
        )}
        {item.result !== undefined && (
          <div className="tool-section">
            <div className="tool-sec-head flex items-center justify-between">
              <span className="tool-sec-label sidebar-section-label">
                Output
              </span>
              <Button variant="link" size="xs" className="tool-copy-btn" onClick={copyOutput}>
                {copied ? "Copied" : "Copy output"}
              </Button>
            </div>
            <pre className="result scrollbar-thin mt-1 overflow-x-auto rounded-md bg-background/70 p-2 font-mono text-xs">
              {item.result}
            </pre>
          </div>
        )}
      </div>
    </details>
  );
}

const SUBAGENT_BADGE: Record<string, { label: string; variant: "primary" | "success" | "destructive" | "warning" | "default" }> = {
  running: { label: "◉ Running", variant: "primary" },
  completed: { label: "✓ Completed", variant: "success" },
  failed: { label: "✗ Failed", variant: "destructive" },
  budget_exceeded: { label: "⚠ Budget Exceeded", variant: "warning" },
  cancelled: { label: "⊘ Cancelled", variant: "default" },
};

function SubagentCard({
  item,
  onInspectTrace,
}: {
  item: Extract<ThreadItem, { kind: "subagent" }>;
  onInspectTrace?: (id: string) => void;
}) {
  const isRunning = item.status === "running";
  const badge = SUBAGENT_BADGE[item.status] ?? { label: item.status, variant: "default" as const };

  return (
    <div
      data-testid="subagent-card"
      className={cn(
        "subagent-card rounded-lg border bg-card/50 p-3",
        `subagent-${item.status}`,
        isRunning ? "border-primary/40" : "border-border",
      )}
    >
      <div className="subagent-header flex items-center justify-between gap-2">
        <div className="subagent-title-area flex items-center gap-2">
          <span className="subagent-icon">🤖</span>
          <span className="subagent-role text-sm font-medium">{item.profileName}</span>
          <Badge variant={badge.variant} className={cn("subagent-badge", `badge-${item.status}`)}>
            {badge.label}
          </Badge>
        </div>
        <div className="subagent-actions flex items-center gap-2">
          <span className="subagent-tokens text-xs text-muted-foreground">{item.tokensUsed} tokens</span>
          {onInspectTrace && (
            <Button
              variant="link"
              size="xs"
              data-testid="inspect-subagent-trace"
              className="subagent-inspect-btn touch-target"
              onClick={() => onInspectTrace(item.subagentId)}
              title="Inspect Subagent Trace"
            >
              Inspect trace →
            </Button>
          )}
        </div>
      </div>
      <div className="subagent-goal mt-2 flex gap-2 text-sm">
        <span className="subagent-label shrink-0 text-muted-foreground">Goal:</span>
        <span className="subagent-goal-text">{item.goal}</span>
      </div>
      {isRunning ? (
        <div className="subagent-live mt-2 flex items-center gap-2 text-xs text-primary">
          <span className="subagent-live-pulse size-1.5 animate-pulse rounded-full bg-primary" />
          <span className="subagent-live-text">
            Step {item.step} • {item.lastAction}
          </span>
        </div>
      ) : item.summary ? (
        <div className="subagent-summary mt-2 rounded-md bg-background/60 p-2">
          <div className="subagent-summary-label sidebar-section-label">
            Summary
          </div>
          <div className="subagent-summary-content mt-1 text-sm">{item.summary}</div>
        </div>
      ) : null}
    </div>
  );
}

function SubagentTraceDrawer({ subagent, onClose }: { subagent: SubagentDetail; onClose: () => void }) {
  const badge = SUBAGENT_BADGE[subagent.status] ?? { label: subagent.status, variant: "default" as const };

  return (
    <div
      className="subagent-drawer-backdrop fixed inset-0 z-50 flex justify-end bg-black/50"
      data-testid="subagent-drawer-backdrop"
      onClick={onClose}
    >
      <div
        className="subagent-drawer scrollbar-thin h-full w-full max-w-[32rem] overflow-y-auto border-l border-border bg-popover p-4 shadow-2xl safe-top safe-bottom"
        data-testid="subagent-drawer"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="subagent-drawer-head flex items-center justify-between">
          <div className="subagent-drawer-title-area flex items-center gap-2">
            <span className="subagent-icon">🤖</span>
            <h3 className="text-sm font-semibold">{subagent.profileName}</h3>
            <Badge variant={badge.variant} className={cn("subagent-badge", `badge-${subagent.status}`)}>
              {badge.label}
            </Badge>
          </div>
          <Button
            variant="ghost"
            size="icon-sm"
            className="subagent-drawer-close"
            data-testid="close-subagent-drawer"
            onClick={onClose}
            title="Close inspector"
          >
            <X className="size-4" />
          </Button>
        </div>

        <div className="subagent-drawer-meta mt-4 grid grid-cols-2 gap-3">
          <div className="subagent-drawer-meta-item rounded-md border border-border bg-card/40 p-2">
            <span className="meta-label sidebar-section-label">Goal</span>
            <div className="meta-val mt-1 text-sm">{subagent.goal}</div>
          </div>
          <div className="subagent-drawer-meta-item rounded-md border border-border bg-card/40 p-2">
            <span className="meta-label sidebar-section-label">Tokens</span>
            <div className="meta-val mt-1 text-sm">{subagent.tokensUsed} tokens consumed</div>
          </div>
        </div>

        {subagent.summary && (
          <div className="subagent-drawer-summary mt-4 rounded-md border border-border bg-card/40 p-3">
            <div className="summary-title text-xs font-medium text-muted-foreground">Final Result / Synthesis</div>
            <div className="summary-body mt-1 text-sm">{subagent.summary}</div>
          </div>
        )}

        <div className="subagent-drawer-timeline-head mt-5">
          <h4 className="sidebar-section-label">
            Execution Trace ({subagent.trace.length} steps)
          </h4>
        </div>

        <div className="subagent-drawer-timeline mt-2 space-y-2">
          {subagent.trace.map((entry, idx) => (
            <div key={idx} className="subagent-trace-step flex gap-3">
              <div className="step-marker flex size-6 shrink-0 items-center justify-center rounded-full bg-secondary font-mono text-xs">
                {entry.step}
              </div>
              <div className="step-content min-w-0 flex-1 rounded-md border border-border bg-card/40 p-2">
                <div className="step-header flex items-center justify-between gap-2">
                  <span className="step-action truncate text-sm">{entry.action}</span>
                  <span className="step-time shrink-0 font-mono text-xs text-muted-foreground">
                    {entry.timestamp}
                  </span>
                </div>
                <div className="step-tokens mt-1 text-xs text-muted-foreground">{entry.tokensUsed} tokens</div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function Item({
  item,
  isStreaming,
  onInspectTrace,
}: {
  item: ThreadItem;
  isStreaming: boolean;
  onInspectTrace?: (id: string) => void;
}) {
  switch (item.kind) {
    case "user":
      return (
        <div data-testid="user-msg" className="msg user leading-relaxed">
          {item.text}
        </div>
      );
    case "assistant":
      return (
        <div
          data-testid="assistant-msg"
          className="msg assistant prose-invert max-w-none text-sm leading-relaxed [&_pre]:my-0 [&_p]:my-2"
        >
          <Markdown remarkPlugins={[remarkGfm]} components={{ code: CodeBlock }}>
            {item.text}
          </Markdown>
        </div>
      );
    case "reasoning":
      return (
        <details className="reasoning rounded-lg border border-border/60 bg-card/40 px-3 py-2 text-sm text-muted-foreground" open={isStreaming ? true : undefined}>
          <summary className="flex cursor-pointer list-none items-center gap-2 text-xs [&::-webkit-details-marker]:hidden">
            <span>Thinking</span>
            {isStreaming && <span className="thinking-pulse size-1.5 animate-pulse rounded-full bg-tool" />}
          </summary>
          <div className="mt-2 whitespace-pre-wrap">{item.text}</div>
        </details>
      );
    case "plan":
      return <PlanTimeline plan={item.plan} />;
    case "tool":
      return <ToolCard item={item} />;
    case "subagent":
      return <SubagentCard item={item} onInspectTrace={onInspectTrace} />;
  }
}

export function Thread({ thread, isMobile }: { thread: ThreadState; isMobile?: boolean }) {
  const { meta, loadOlder, loadingOlder } = useApp();
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const [activeSubagentId, setActiveSubagentId] = useState<string | null>(null);

  useEffect(() => {
    if (thread.selectedSubagentId) {
      setActiveSubagentId(thread.selectedSubagentId);
    }
  }, [thread.selectedSubagentId]);

  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && stickToBottom.current) el.scrollTop = el.scrollHeight;
  }, [thread.items, thread.notices, thread.pending.length, thread.plan]);

  useEffect(() => {
    stickToBottom.current = true;
  }, [meta?.directory]);

  return (
    <div
      ref={scroller}
      className={cn(
        "thread scrollbar-thin flex min-w-0 flex-1 flex-col gap-3 overflow-y-auto",
        isMobile ? "px-3 py-3" : "px-6 py-4",
      )}
      data-testid="thread"
      onScroll={(e) => {
        const el = e.currentTarget;
        stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      }}
    >
      {meta && meta.loadedStart > 0 && (
        <Button
          data-testid="load-older"
          variant="ghost"
          size="sm"
          className="self-center text-xs"
          disabled={loadingOlder}
          onClick={() => void loadOlder()}
        >
          {loadingOlder ? "Loading…" : `Load earlier messages (${meta.loadedStart} more)`}
        </Button>
      )}
      {thread.items.length === 0 && !thread.running && (!meta || meta.loadedStart === 0) && (
        <div className="thread-empty m-auto max-w-md rounded-xl border border-dashed border-border bg-card/30 p-8 text-center text-sm text-muted-foreground">
          <strong className="mb-1 block text-base text-foreground">What should the agent do?</strong>
          Describe a task below. It works in this project and asks before risky actions.
        </div>
      )}
      {thread.plan && <PlanTimeline plan={thread.plan} />}
      {thread.items.map((item, idx) => (
        <Item
          key={item.id}
          item={item}
          isStreaming={thread.running && idx === thread.items.length - 1}
          onInspectTrace={(id) => setActiveSubagentId(id)}
        />
      ))}
      {thread.queue && thread.queue.length > 0 && (
        <div className="thread-queued-box rounded-lg border border-border bg-card/40 p-3" data-testid="thread-queued-box">
          <div className="thread-queued-head">
            <span className="thread-queued-title text-xs font-medium text-muted-foreground">
              Queued next ({thread.queue.length})
            </span>
          </div>
          <div className="thread-queued-items mt-2 space-y-1">
            {thread.queue.map((promptText, qIdx) => (
              <div key={qIdx} className="thread-queued-item flex gap-2 text-sm">
                <span className="thread-queued-index font-mono text-xs text-muted-foreground">#{qIdx + 1}</span>
                <span className="thread-queued-text">{promptText}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      {thread.running && thread.pending.length === 0 && (
        <div className="working flex items-center gap-2 text-sm text-muted-foreground" data-testid="working">
          <span className="size-1.5 animate-pulse rounded-full bg-primary" />
          Thinking…
        </div>
      )}
      {activeSubagentId && thread.subagents[activeSubagentId] && (
        <SubagentTraceDrawer subagent={thread.subagents[activeSubagentId]} onClose={() => setActiveSubagentId(null)} />
      )}
    </div>
  );
}
