import { useEffect, useLayoutEffect, useRef, useState } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useApp } from "../state/store";
import type { ThreadItem, ThreadState } from "../state/thread";
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

function Item({ item, isStreaming }: { item: ThreadItem; isStreaming: boolean }) {
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
  }
}

export function Thread({ thread }: { thread: ThreadState }) {
  const { meta, loadOlder, loadingOlder } = useApp();
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);

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
    </div>
  );
}
