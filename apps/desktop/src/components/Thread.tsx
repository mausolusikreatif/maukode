import { useEffect, useLayoutEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useApp } from "../state/store";
import type { SubagentDetail, ThreadItem, ThreadState } from "../state/thread";
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
      <code className={className} {...props}>
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
    <div className="code-block">
      <div className="code-head">
        <span className="code-lang">{match ? match[1] : "text"}</span>
        <button type="button" className="code-copy-btn" onClick={copy} title="Copy code">
          {copied ? "Copied!" : "Copy"}
        </button>
      </div>
      <pre>
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
    <details data-testid="tool-card" className={`tool ${item.status}`} open={item.status === "running" ? true : undefined}>
      <summary>
        <span className="tool-name">{item.name || "tool"}</span>
        <span className="tool-hint">{argHint(item.args ?? "")}</span>
        <span data-testid="tool-status" className="tool-status">{label}</span>
      </summary>
      <div className="tool-body">
        {item.args && (
          <div className="tool-section">
            <span className="tool-sec-label">Input</span>
            <pre className="args">{prettyArgs(item.args)}</pre>
          </div>
        )}
        {item.result !== undefined && (
          <div className="tool-section">
            <div className="tool-sec-head">
              <span className="tool-sec-label">Output</span>
              <button type="button" className="link tool-copy-btn" onClick={copyOutput}>
                {copied ? "Copied" : "Copy output"}
              </button>
            </div>
            <pre className="result">{item.result}</pre>
          </div>
        )}
      </div>
    </details>
  );
}

function SubagentCard({
  item,
  onInspectTrace,
}: {
  item: Extract<ThreadItem, { kind: "subagent" }>;
  onInspectTrace?: (id: string) => void;
}) {
  const isRunning = item.status === "running";
  const badgeLabel = {
    running: "◉ Running",
    completed: "✓ Completed",
    failed: "✗ Failed",
    budget_exceeded: "⚠ Budget Exceeded",
    cancelled: "⊘ Cancelled",
  }[item.status] ?? item.status;

  return (
    <div
      data-testid="subagent-card"
      className={`subagent-card subagent-${item.status}`}
    >
      <div className="subagent-header">
        <div className="subagent-title-area">
          <span className="subagent-icon">🤖</span>
          <span className="subagent-role">{item.profileName}</span>
          <span className={`subagent-badge badge-${item.status}`}>
            {badgeLabel}
          </span>
        </div>
        <div className="subagent-actions">
          <span className="subagent-tokens">{item.tokensUsed} tokens</span>
          {onInspectTrace && (
            <button
              type="button"
              data-testid="inspect-subagent-trace"
              className="subagent-inspect-btn"
              onClick={() => onInspectTrace(item.subagentId)}
              title="Inspect Subagent Trace"
            >
              Inspect trace →
            </button>
          )}
        </div>
      </div>
      <div className="subagent-goal">
        <span className="subagent-label">Goal:</span>
        <span className="subagent-goal-text">{item.goal}</span>
      </div>
      {isRunning ? (
        <div className="subagent-live">
          <span className="subagent-live-pulse" />
          <span className="subagent-live-text">
            Step {item.step} • {item.lastAction}
          </span>
        </div>
      ) : item.summary ? (
        <div className="subagent-summary">
          <div className="subagent-summary-label">Summary</div>
          <div className="subagent-summary-content">{item.summary}</div>
        </div>
      ) : null}
    </div>
  );
}

function SubagentTraceDrawer({
  subagent,
  onClose,
}: {
  subagent: SubagentDetail;
  onClose: () => void;
}) {
  const badgeLabel = {
    running: "◉ Running",
    completed: "✓ Completed",
    failed: "✗ Failed",
    budget_exceeded: "⚠ Budget Exceeded",
    cancelled: "⊘ Cancelled",
  }[subagent.status] ?? subagent.status;

  return (
    <div
      className="subagent-drawer-backdrop"
      data-testid="subagent-drawer-backdrop"
      onClick={onClose}
    >
      <div
        className="subagent-drawer"
        data-testid="subagent-drawer"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="subagent-drawer-head">
          <div className="subagent-drawer-title-area">
            <span className="subagent-icon">🤖</span>
            <h3>{subagent.profileName}</h3>
            <span className={`subagent-badge badge-${subagent.status}`}>
              {badgeLabel}
            </span>
          </div>
          <button
            type="button"
            className="subagent-drawer-close"
            data-testid="close-subagent-drawer"
            onClick={onClose}
            title="Close inspector"
          >
            ✕
          </button>
        </div>

        <div className="subagent-drawer-meta">
          <div className="subagent-drawer-meta-item">
            <span className="meta-label">Goal</span>
            <div className="meta-val">{subagent.goal}</div>
          </div>
          <div className="subagent-drawer-meta-item">
            <span className="meta-label">Tokens</span>
            <div className="meta-val">{subagent.tokensUsed} tokens consumed</div>
          </div>
        </div>

        {subagent.summary && (
          <div className="subagent-drawer-summary">
            <div className="summary-title">Final Result / Synthesis</div>
            <div className="summary-body">{subagent.summary}</div>
          </div>
        )}

        <div className="subagent-drawer-timeline-head">
          <h4>Execution Trace ({subagent.trace.length} steps)</h4>
        </div>

        <div className="subagent-drawer-timeline">
          {subagent.trace.map((entry, idx) => (
            <div key={idx} className="subagent-trace-step">
              <div className="step-marker">{entry.step}</div>
              <div className="step-content">
                <div className="step-header">
                  <span className="step-action">{entry.action}</span>
                  <span className="step-time">{entry.timestamp}</span>
                </div>
                <div className="step-tokens">{entry.tokensUsed} tokens</div>
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
      return <div data-testid="user-msg" className="msg user">{item.text}</div>;
    case "assistant":
      return (
        <div data-testid="assistant-msg" className="msg assistant">
          <Markdown
            remarkPlugins={[remarkGfm]}
            components={{
              code: CodeBlock,
            }}
          >
            {item.text}
          </Markdown>
        </div>
      );
    case "reasoning":
      return (
        <details className="reasoning" open={isStreaming ? true : undefined}>
          <summary>
            <span>Thinking</span>
            {isStreaming && <span className="thinking-pulse" />}
          </summary>
          <div>{item.text}</div>
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

export function Thread({ thread }: { thread: ThreadState }) {
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
      className="thread"
      data-testid="thread"
      onScroll={(e) => {
        const el = e.currentTarget;
        stickToBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
      }}
    >
      {meta && meta.loadedStart > 0 && (
        <button data-testid="load-older" className="link" disabled={loadingOlder} onClick={() => void loadOlder()}>
          {loadingOlder ? "Loading…" : `Load earlier messages (${meta.loadedStart} more)`}
        </button>
      )}
      {thread.items.length === 0 && !thread.running && (!meta || meta.loadedStart === 0) && (
        <div className="thread-empty">
          <strong>What should the agent do?</strong>
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
        <div className="thread-queued-box" data-testid="thread-queued-box">
          <div className="thread-queued-head">
            <span className="thread-queued-title">Queued next ({thread.queue.length})</span>
          </div>
          <div className="thread-queued-items">
            {thread.queue.map((promptText, qIdx) => (
              <div key={qIdx} className="thread-queued-item">
                <span className="thread-queued-index">#{qIdx + 1}</span>
                <span className="thread-queued-text">{promptText}</span>
              </div>
            ))}
          </div>
        </div>
      )}
      {thread.running && thread.pending.length === 0 && <div className="working muted" data-testid="working">Working…</div>}
      {activeSubagentId && thread.subagents[activeSubagentId] && (
        <SubagentTraceDrawer
          subagent={thread.subagents[activeSubagentId]}
          onClose={() => setActiveSubagentId(null)}
        />
      )}
    </div>
  );
}
