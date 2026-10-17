import { useEffect, useMemo, useRef, useState } from "react";
import { useApp } from "../state/store";
import type { ThreadState } from "../state/thread";
import {
  buildAutocompleteItems,
  estimateTokens,
  parseRepoMap,
  type AutocompleteItem,
  type ContextChip,
  type ParsedCodebase,
} from "../components/composerContext";

const MENTION = /(?:^|\s)@([a-zA-Z0-9_\-./]*)$/;
const NO_CODEBASE: ParsedCodebase = { files: [], folders: [], symbols: [] };

export function Composer({ thread }: { thread: ThreadState }) {
  const { send, queue, steer, stop, clearQueue } = useApp.getState();
  const client = useApp((s) => s.client);
  const activeId = useApp((s) => s.activeId);
  const changed = useApp((s) => s.review.files);
  const [text, setText] = useState("");
  const [chips, setChips] = useState<ContextChip[]>([]);
  const [codebase, setCodebase] = useState<ParsedCodebase>(NO_CODEBASE);
  // Which session the cached repo map belongs to (a ref: changing it must not re-run the loader).
  const loadedFor = useRef<string | null>(null);
  const activeRef = useRef(activeId);
  activeRef.current = activeId;
  const [query, setQuery] = useState<string | null>(null);
  const [pick, setPick] = useState(0);
  const ref = useRef<HTMLTextAreaElement>(null);
  const running = thread.running;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 160)}px`;
  }, [text]);

  useEffect(() => {
    setCodebase(NO_CODEBASE);
    loadedFor.current = null;
  }, [activeId]);

  // The repo map is only needed once the user types "@", and is cached per session.
  useEffect(() => {
    if (query === null || !client || !activeId || loadedFor.current === activeId) return;
    loadedFor.current = activeId;
    const id = activeId;
    client
      .codebaseMap(id, 1000)
      .then((r) => {
        if (r?.map && activeRef.current === id) setCodebase(parseRepoMap(r.map));
      })
      .catch(() => { loadedFor.current = null; });
  }, [query, client, activeId]);

  const items = useMemo<AutocompleteItem[]>(() => {
    if (query === null) return [];
    return buildAutocompleteItems({
      query,
      mode: "all",
      codebase,
      changedFiles: changed.map((f) => ({ path: f.path, status: f.status })),
      diffCount: changed.length,
    }).filter((i) => i.kind !== "action");
  }, [query, codebase, changed]);

  const tokens = estimateTokens(text, chips);

  const onText = (value: string, cursor: number) => {
    setText(value);
    const m = MENTION.exec(value.slice(0, cursor));
    setQuery(m ? m[1] : null);
    setPick(0);
  };

  const choose = (item: AutocompleteItem) => {
    const el = ref.current;
    const cursor = el?.selectionStart ?? text.length;
    const before = text.slice(0, cursor);
    const m = MENTION.exec(before);
    const start = m ? m.index + (m[0].startsWith("@") ? 0 : 1) : before.length;
    setText(text.slice(0, start) + text.slice(cursor));
    setChips((c) =>
      c.some((x) => x.kind === item.kind && x.value === item.value)
        ? c
        : [...c, { id: item.id, kind: item.kind as ContextChip["kind"], value: item.value, label: `@${item.value}` }],
    );
    setQuery(null);
    el?.focus();
  };

  const submit = () => {
    const t = text.trim();
    if (!t && chips.length === 0) return;
    const tags = chips.map((c) => `@${c.value}`).join(" ");
    const body = tags ? (t ? `${tags}\n\n${t}` : tags) : t;
    setText("");
    setChips([]);
    setQuery(null);
    if (running) void queue(body);
    else void send(body);
  };

  const empty = !text.trim() && chips.length === 0;

  return (
    <div className="composer">
      {thread.queue.length > 0 && (
        <div className="queue">
          <span>{thread.queue.length} pesan antre, dikirim setelah giliran ini</span>
          <button className="btn quiet small" onClick={() => void clearQueue()}>Kosongkan</button>
        </div>
      )}
      {chips.length > 0 && (
        <div className="ctx-chips" aria-label="Konteks terlampir">
          {chips.map((c) => (
            <span key={c.id} className="chip">
              <span className="trunc">{c.label}</span>
              <button aria-label={`Lepas ${c.label}`} onClick={() => setChips((x) => x.filter((y) => y.id !== c.id))}>x</button>
            </span>
          ))}
        </div>
      )}
      <div className="mention-wrap">
        {query !== null && (
          <ul className="mention" role="listbox" aria-label="Rujukan konteks">
            {items.length === 0 && <li className="muted small" style={{ padding: "0.5rem 0.75rem" }}>Tidak ada file, folder, atau simbol yang cocok.</li>}
            {items.map((it, i) => (
              <li key={it.id} role="option" aria-selected={i === pick}>
                <button
                  className="mention-item" data-active={i === pick}
                  onMouseDown={(e) => { e.preventDefault(); choose(it); }}
                >
                  <span className="trunc">{it.label}</span>
                  <span className="small muted trunc">{it.description}</span>
                </button>
              </li>
            ))}
          </ul>
        )}
        <textarea
          ref={ref} rows={1} value={text} aria-label="Pesan untuk agent"
          placeholder={running ? "Agent sedang bekerja. Pesan baru masuk antrean." : "Tulis tugas. Ketik @ untuk melampirkan file, folder, atau perubahan."}
          onChange={(e) => onText(e.target.value, e.target.selectionStart)}
          onKeyDown={(e) => {
            if (query !== null && items.length > 0) {
              if (e.key === "ArrowDown") { e.preventDefault(); setPick((p) => (p + 1) % items.length); return; }
              if (e.key === "ArrowUp") { e.preventDefault(); setPick((p) => (p - 1 + items.length) % items.length); return; }
              if (e.key === "Enter" || e.key === "Tab") { e.preventDefault(); choose(items[pick]); return; }
            }
            if (e.key === "Escape" && query !== null) { setQuery(null); return; }
            const touch = window.matchMedia("(pointer: coarse)").matches;
            if (e.key === "Enter" && !e.shiftKey && !touch) {
              e.preventDefault();
              submit();
            }
          }}
        />
      </div>
      <div className="composer-row">
        <span className="small muted">
          {running ? "berjalan" : "siap"}
          {tokens.tokens > 0 ? `, ±${tokens.tokens} token` : ""}
        </span>
        <span style={{ display: "flex", gap: 6 }}>
          <button
            className="btn" aria-label="Lampirkan konteks"
            onClick={() => { const el = ref.current; const v = text.endsWith(" ") || text === "" ? `${text}@` : `${text} @`; onText(v, v.length); el?.focus(); }}
          >
            @
          </button>
          {running && (
            <>
              <button className="btn" disabled={!text.trim()} onClick={() => { const t = text.trim(); setText(""); void steer(t); }}>
                Arahkan
              </button>
              <button className="btn deny" onClick={() => void stop()}>Hentikan</button>
            </>
          )}
          <button className="btn send" disabled={empty} onClick={submit}>
            {running ? "Antre" : "Kirim"}
          </button>
        </span>
      </div>
    </div>
  );
}
