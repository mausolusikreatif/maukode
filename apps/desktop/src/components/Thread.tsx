import { useEffect, useLayoutEffect, useRef } from "react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useApp } from "../state/store";
import type { ThreadItem, ThreadState } from "../state/thread";

function prettyArgs(args: string): string {
  try {
    return JSON.stringify(JSON.parse(args), null, 2);
  } catch {
    return args;
  }
}

function ToolCard({ item }: { item: Extract<ThreadItem, { kind: "tool" }> }) {
  const label = { running: "running…", done: "done", error: "failed", private: "shown only to you" }[item.status];
  return (
    <details data-testid="tool-card" className={`tool ${item.status}`}>
      <summary>
        <span className="tool-name">{item.name || "tool"}</span>
        <span data-testid="tool-status" className="tool-status">{label}</span>
      </summary>
      {item.args && <pre className="args">{prettyArgs(item.args)}</pre>}
      {item.result !== undefined && <pre className="result">{item.result}</pre>}
    </details>
  );
}

function Item({ item }: { item: ThreadItem }) {
  switch (item.kind) {
    case "user":
      return <div data-testid="user-msg" className="msg user">{item.text}</div>;
    case "assistant":
      return (
        <div data-testid="assistant-msg" className="msg assistant">
          <Markdown remarkPlugins={[remarkGfm]}>{item.text}</Markdown>
        </div>
      );
    case "reasoning":
      return (
        <details className="reasoning">
          <summary>Thinking</summary>
          <div>{item.text}</div>
        </details>
      );
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
  }, [thread.items, thread.notices, thread.pending.length]);

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
      {thread.items.map((item) => (
        <Item key={item.id} item={item} />
      ))}
      {thread.running && thread.pending.length === 0 && <div className="working muted" data-testid="working">Working…</div>}
    </div>
  );
}
