import { useEffect, useMemo, useRef, useState } from "react";
import { useApp } from "../state/store";
import type { ThreadState } from "../state/thread";
import { usageText } from "../state/format";
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

/** Image types and size the daemon accepts (see `IMAGE_MEDIA_TYPES` / `MAX_IMAGE_BYTES`). */
const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];
const MAX_IMAGE_BYTES = 5 * 1024 * 1024;
const MAX_IMAGES = 20;

interface Attached {
  id: string;
  name: string;
  media_type: string;
  /** Base64 without the `data:` prefix. */
  data: string;
}

function readImage(file: File): Promise<Attached> {
  return new Promise((resolve, reject) => {
    if (!IMAGE_TYPES.includes(file.type)) {
      reject(new Error(`${file.name}: hanya PNG, JPEG, GIF, atau WebP`));
      return;
    }
    if (file.size > MAX_IMAGE_BYTES) {
      reject(new Error(`${file.name}: lebih dari 5 MB`));
      return;
    }
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(`${file.name}: gagal dibaca`));
    reader.onload = () => {
      const url = String(reader.result);
      resolve({
        id: `${file.name}-${file.size}-${Date.now()}-${Math.random()}`,
        name: file.name || "gambar",
        media_type: file.type,
        data: url.slice(url.indexOf(",") + 1),
      });
    };
    reader.readAsDataURL(file);
  });
}

export function Composer({ thread }: { thread: ThreadState }) {
  const { send, queue, steer, stop, clearQueue } = useApp.getState();
  const client = useApp((s) => s.client);
  const activeId = useApp((s) => s.activeId);
  const changed = useApp((s) => s.review.files);
  const [text, setText] = useState("");
  const [chips, setChips] = useState<ContextChip[]>([]);
  const [images, setImages] = useState<Attached[]>([]);
  const [imageError, setImageError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const [codebase, setCodebase] = useState<ParsedCodebase>(NO_CODEBASE);
  // Which session the cached repo map belongs to (a ref: changing it must not re-run the loader).
  const loadedFor = useRef<string | null>(null);
  const activeRef = useRef(activeId);
  activeRef.current = activeId;
  const [mapState, setMapState] = useState<"idle" | "loading" | "ready" | "failed">("idle");
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
    setMapState("idle");
    loadedFor.current = null;
  }, [activeId]);

  // The repo map is only needed once the user types "@", and is cached per session.
  useEffect(() => {
    if (query === null || !client || !activeId || loadedFor.current === activeId) return;
    loadedFor.current = activeId;
    const id = activeId;
    setMapState("loading");
    // Huge folders (e.g. a home directory) can take a minute to scan: give up waiting after 12s.
    const timeout = new Promise<null>((r) => setTimeout(() => r(null), 12000));
    void Promise.race([client.codebaseMap(id, 1000).catch(() => null), timeout]).then((r) => {
      if (activeRef.current !== id) return;
      const parsed = r?.map ? parseRepoMap(r.map) : NO_CODEBASE;
      setCodebase(parsed);
      setMapState(parsed.files.length > 0 ? "ready" : "failed");
    });
  }, [query, client, activeId]);

  const items = useMemo<AutocompleteItem[]>(() => {
    if (query === null) return [];
    const found = buildAutocompleteItems({
      query,
      mode: "all",
      codebase,
      changedFiles: changed.map((f) => ({ path: f.path, status: f.status })),
      diffCount: changed.length,
    }).filter((i) => i.kind !== "action");
    const q = query.trim();
    // Always allow the typed path as-is, so @ works even when no file list is available.
    if (q && !found.some((i) => i.value === q)) {
      found.push({ id: `manual-${q}`, kind: "file", value: q, label: `@${q}`, description: "Pakai path ini apa adanya", icon: "📄" });
    }
    return found;
  }, [query, codebase, changed]);

  const tokens = estimateTokens(text, chips);

  const attach = (files: Iterable<File>) => {
    const list = Array.from(files).filter((f) => f.type.startsWith("image/"));
    if (list.length === 0) return;
    setImageError(null);
    void Promise.allSettled(list.map(readImage)).then((results) => {
      const ok = results.flatMap((r) => (r.status === "fulfilled" ? [r.value] : []));
      const failed = results.flatMap((r) => (r.status === "rejected" ? [String((r.reason as Error).message)] : []));
      setImages((current) => {
        const room = Math.max(0, MAX_IMAGES - current.length);
        if (ok.length > room) failed.push(`Maksimal ${MAX_IMAGES} gambar per pesan`);
        return [...current, ...ok.slice(0, room)];
      });
      if (failed.length > 0) setImageError(failed.join("; "));
    });
  };

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
    if (!t && chips.length === 0 && images.length === 0) return;
    // Images go with a new turn only; the queue holds text.
    if (running && images.length > 0) return;
    const tags = chips.map((c) => `@${c.value}`).join(" ");
    const body = tags ? (t ? `${tags}\n\n${t}` : tags) : t;
    const attached = images.map(({ media_type, data }) => ({ media_type, data }));
    setText("");
    setChips([]);
    setImages([]);
    setImageError(null);
    setQuery(null);
    if (running) void queue(body);
    else void send(body, attached);
  };

  const empty = !text.trim() && chips.length === 0 && images.length === 0;
  const blocked = running && images.length > 0;
  const usage = usageText(thread.sessionUsage);

  return (
    <div
      className="composer"
      onDragOver={(e) => { if (e.dataTransfer.types.includes("Files")) e.preventDefault(); }}
      onDrop={(e) => {
        if (e.dataTransfer.files.length === 0) return;
        e.preventDefault();
        attach(e.dataTransfer.files);
      }}
    >
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
      {images.length > 0 && (
        <div className="thumbs" aria-label="Gambar terlampir">
          {images.map((img) => (
            <span key={img.id} className="thumb-wrap">
              <img className="thumb" src={`data:${img.media_type};base64,${img.data}`} alt={img.name} />
              <button aria-label={`Lepas ${img.name}`} onClick={() => setImages((x) => x.filter((y) => y.id !== img.id))}>x</button>
            </span>
          ))}
        </div>
      )}
      {imageError && <div className="small" style={{ color: "var(--red)" }}>{imageError}</div>}
      {blocked && <div className="small muted">Gambar hanya bisa dikirim saat agent tidak sedang bekerja.</div>}
      <div className="mention-wrap">
        {query !== null && (
          <ul className="mention" role="listbox" aria-label="Rujukan konteks">
            {mapState === "loading" && <li className="muted small" style={{ padding: "0.5rem 0.75rem" }}>Memuat daftar file proyek…</li>}
            {mapState === "failed" && <li className="muted small" style={{ padding: "0.5rem 0.75rem" }}>Daftar file tidak tersedia untuk folder ini (kosong atau terlalu besar). Ketik path lengkapnya setelah @.</li>}
            {mapState === "ready" && items.length === 0 && <li className="muted small" style={{ padding: "0.5rem 0.75rem" }}>Tidak ada file, folder, atau simbol yang cocok.</li>}
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
          onPaste={(e) => {
            const files = Array.from(e.clipboardData.files).filter((f) => f.type.startsWith("image/"));
            if (files.length === 0) return;
            e.preventDefault();
            attach(files);
          }}
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
          {usage && <span title="Pemakaian sesi ini sejauh ini"> · sesi {usage}</span>}
        </span>
        <span style={{ display: "flex", gap: 6 }}>
          <button
            className="btn" aria-label="Lampirkan konteks"
            onClick={() => { const el = ref.current; const v = text.endsWith(" ") || text === "" ? `${text}@` : `${text} @`; onText(v, v.length); el?.focus(); }}
          >
            @
          </button>
          <input
            ref={fileInput} type="file" accept={IMAGE_TYPES.join(",")} multiple hidden
            onChange={(e) => { if (e.target.files) attach(e.target.files); e.target.value = ""; }}
          />
          <button className="btn" aria-label="Lampirkan gambar" title="Lampirkan gambar (atau tempel / seret ke sini)" onClick={() => fileInput.current?.click()}>
            Gambar
          </button>
          {running && (
            <>
              <button className="btn" disabled={!text.trim()} onClick={() => { const t = text.trim(); setText(""); void steer(t); }}>
                Arahkan
              </button>
              <button className="btn deny" onClick={() => void stop()}>Hentikan</button>
            </>
          )}
          <button className="btn send" disabled={empty || blocked} onClick={submit}>
            {running ? "Antre" : "Kirim"}
          </button>
        </span>
      </div>
    </div>
  );
}
