import { useEffect, useRef } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ThreadItem, ThreadState } from "../state/thread";
import { useApp } from "../state/store";

function Md({ text }: { text: string }) {
  return (
    <div className="prose">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
    </div>
  );
}

/** One-line summary of a tool call: the command or path it acts on. */
function summarize(args: string): string {
  try {
    const o = JSON.parse(args) as Record<string, unknown>;
    for (const k of ["command", "path", "file_path", "pattern", "query", "url"]) {
      if (typeof o[k] === "string") return o[k] as string;
    }
    const first = Object.values(o).find((v) => typeof v === "string");
    return typeof first === "string" ? first : "";
  } catch {
    return args.slice(0, 120);
  }
}

/** Tools whose `path` argument is a file worth opening. */
const FILE_TOOLS = new Set(["read_file", "write_file", "edit_file"]);

/** The file a tool call worked on, if it names one. */
function fileOf(tool: string, args: string): string | null {
  if (!FILE_TOOLS.has(tool)) return null;
  try {
    const path = (JSON.parse(args) as Record<string, unknown>).path;
    return typeof path === "string" && path ? path : null;
  } catch {
    return null;
  }
}

function Item({ item }: { item: ThreadItem }) {
  switch (item.kind) {
    case "user":
      return (
        <div className="row user">
          <span className="gutter">&gt;</span>
          <div className="body">{item.text}</div>
        </div>
      );
    case "image":
      return (
        <div className="row user">
          <span className="gutter" />
          <img className="thumb large" src={`data:${item.mediaType};base64,${item.data}`} alt="Gambar terlampir" />
        </div>
      );
    case "assistant":
      return (
        <div className="row">
          <span className="gutter" />
          <Md text={item.text} />
        </div>
      );
    case "reasoning":
      return (
        <div className="row">
          <span className="gutter" />
          <details className="think">
            <summary>Proses berpikir</summary>
            <Md text={item.text} />
          </details>
        </div>
      );
    case "tool":
      return (
        <div className="row">
          <span className="gutter">{item.status === "error" ? "!" : "$"}</span>
          <details className="tool" data-status={item.status}>
            <summary>
              <span className="name">{item.name}</span>
              <span className="sum trunc">{summarize(item.args)}</span>
            </summary>
            {fileOf(item.name, item.args) && (
              <button
                className="btn quiet small" title="Buka di editor pada mesin tempat daemon berjalan"
                onClick={() => void useApp.getState().openInEditor(fileOf(item.name, item.args) as string)}
              >
                Buka di editor
              </button>
            )}
            <pre>{item.args}</pre>
            {item.result && <pre>{item.result.length > 6000 ? `${item.result.slice(0, 6000)}\n… dipotong` : item.result}</pre>}
          </details>
        </div>
      );
    case "plan":
      return (
        <div className="row">
          <span className="gutter" />
          <div className="plan">
            {item.plan.title && <div className="strong">{item.plan.title}</div>}
            {item.plan.steps.map((s) => (
              <div key={s.id} className="step" data-s={s.status}>
                <span>{s.status === "completed" ? "x" : s.status === "in_progress" ? ">" : s.status === "failed" ? "!" : "-"}</span>
                <span>{s.title}</span>
              </div>
            ))}
          </div>
        </div>
      );
    case "subagent":
      return (
        <div className="row">
          <span className="gutter">+</span>
          <div className="small">
            <span className="strong">{item.profileName}</span>{" "}
            <span className="muted">{item.status === "running" ? `langkah ${item.step}: ${item.lastAction}` : item.status}</span>
            <div className="muted trunc">{item.goal}</div>
          </div>
        </div>
      );
  }
}

const STOP_TEXT: Record<string, string> = {
  max_tokens: "Berhenti: batas token tercapai.",
  max_steps: "Berhenti: batas langkah tercapai. Kirim pesan untuk melanjutkan.",
  cancelled: "Dihentikan.",
  error: "Berhenti karena error.",
};

export function Thread({ thread, loadedStart, onLoadOlder }: {
  thread: ThreadState;
  loadedStart: number;
  onLoadOlder: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    const el = ref.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [thread.items, thread.notices, thread.pending.length]);

  return (
    <div
      className="thread" ref={ref}
      onScroll={(e) => {
        const el = e.currentTarget;
        stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 120;
      }}
    >
      <div className="thread-in">
        {loadedStart > 0 && (
          <button className="btn small" style={{ justifySelf: "center" }} onClick={onLoadOlder}>Muat pesan lama</button>
        )}
        {thread.items.length === 0 && (
          <div className="empty">
            <div className="strong" style={{ fontSize: 15 }}>Sesi masih kosong</div>
            <p className="muted" style={{ margin: 0 }}>Tulis tugas di bawah. Agent akan meminta izin sebelum menjalankan perintah berisiko.</p>
          </div>
        )}
        {thread.items.map((i) => <Item key={i.id} item={i} />)}
        {thread.notices.map((n) => (
          <div key={n.id} className={`notice ${n.level === "error" ? "error" : ""}`}>{n.text}</div>
        ))}
        {!thread.running && thread.stop && STOP_TEXT[thread.stop.reason] && (
          <div className="notice">{thread.stop.message || STOP_TEXT[thread.stop.reason]}</div>
        )}
      </div>
    </div>
  );
}
