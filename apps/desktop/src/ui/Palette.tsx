import { useEffect, useMemo, useRef, useState } from "react";
import { useApp } from "../state/store";
import { baseName } from "../state/format";
import { filterCommands, type PaletteCommand } from "../state/palette";
import { primaryMod } from "../platform";
import type { Tab } from "./Sidebar";

/** Command palette (Ctrl/Cmd+K): navigation, session actions, and every session by title. */
export function Palette({ onGo }: { onGo: (tab: Tab) => void }) {
  const open = useApp((s) => s.commandPaletteOpen);
  const sessions = useApp((s) => s.sessions);
  const activeId = useApp((s) => s.activeId);
  const archived = useApp((s) => s.archivedSessionIds);
  const running = useApp((s) => (s.activeId ? s.threads[s.activeId]?.running ?? false : false));
  const reviewOpen = useApp((s) => s.review.open);
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState(0);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) {
      setQuery("");
      setPick(0);
      setTimeout(() => input.current?.focus(), 0);
    }
  }, [open]);

  const commands = useMemo<PaletteCommand[]>(() => {
    const app = useApp.getState();
    const list: PaletteCommand[] = [
      { id: "new", label: "Sesi baru", run: () => onGo("new") },
      { id: "overview", label: "Buka ringkasan", run: () => onGo("overview") },
      { id: "settings", label: "Buka pengaturan", run: () => onGo("settings") },
      { id: "shortcuts", label: "Pintasan keyboard", hint: "?", run: () => app.toggleShortcuts() },
    ];
    if (activeId) {
      list.push({ id: "review", label: reviewOpen ? "Tutup panel perubahan" : "Lihat perubahan", run: () => void app.toggleReview() });
      if (running) list.push({ id: "stop", label: "Hentikan agent", run: () => void app.stop() });
      const isArchived = archived.includes(activeId);
      list.push({
        id: "archive",
        label: isArchived ? "Pulihkan sesi ini dari arsip" : "Arsipkan sesi ini",
        run: () => void app.toggleArchiveSession(activeId),
      });
    }
    for (const s of sessions) {
      if (s.id === activeId) continue;
      list.push({
        id: `open-${s.id}`,
        label: `Buka sesi: ${s.title || "Tanpa judul"}`,
        hint: `${baseName(s.project_dir ?? s.directory)}${s.archived ? " · arsip" : ""}`,
        run: () => void app.openSession(s.id),
      });
    }
    return list;
  }, [sessions, activeId, archived, running, reviewOpen, onGo]);

  const shown = useMemo(() => filterCommands(commands, query).slice(0, 50), [commands, query]);
  if (!open) return null;

  const close = () => useApp.getState().closeCommandPalette();
  const run = (c: PaletteCommand | undefined) => {
    if (!c) return;
    close();
    c.run();
  };

  return (
    <div className="overlay" role="presentation" onMouseDown={close}>
      <div className="palette" role="dialog" aria-label="Palet perintah" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={input} className="field" value={query} aria-label="Cari perintah"
          placeholder={`Ketik perintah atau judul sesi (${primaryMod()}+K)`}
          onChange={(e) => { setQuery(e.target.value); setPick(0); }}
          onKeyDown={(e) => {
            if (e.key === "Escape") { e.preventDefault(); close(); }
            if (e.key === "ArrowDown") { e.preventDefault(); setPick((p) => Math.min(p + 1, Math.max(shown.length - 1, 0))); }
            if (e.key === "ArrowUp") { e.preventDefault(); setPick((p) => Math.max(p - 1, 0)); }
            if (e.key === "Enter") { e.preventDefault(); run(shown[pick]); }
          }}
        />
        <ul role="listbox" aria-label="Perintah">
          {shown.length === 0 && <li className="muted small" style={{ padding: "0.6rem 0.75rem" }}>Tidak ada perintah yang cocok.</li>}
          {shown.map((c, i) => (
            <li key={c.id} role="option" aria-selected={i === pick}>
              <button className="mention-item" data-active={i === pick} onMouseEnter={() => setPick(i)} onClick={() => run(c)}>
                <span className="trunc">{c.label}</span>
                {c.hint && <span className="small muted trunc">{c.hint}</span>}
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** Keyboard shortcut sheet (`?`). */
export function Shortcuts() {
  const open = useApp((s) => s.shortcutsOpen);
  if (!open) return null;
  const mod = primaryMod();
  const rows: [string, string][] = [
    [`${mod}+K`, "Palet perintah: pindah sesi, navigasi, aksi sesi"],
    ["?", "Tampilkan pintasan ini"],
    ["Enter", "Kirim pesan (antre bila agent sedang bekerja)"],
    ["Shift+Enter", "Baris baru"],
    ["@", "Lampirkan file, folder, simbol, atau perubahan"],
    ["Esc", "Tutup dialog atau daftar saran"],
  ];
  const close = () => useApp.getState().closeShortcuts();
  return (
    <div className="overlay" role="presentation" onMouseDown={close}>
      <div className="palette" role="dialog" aria-label="Pintasan keyboard" onMouseDown={(e) => e.stopPropagation()}>
        <div className="strong" style={{ padding: "0.6rem 0.75rem" }}>Pintasan keyboard</div>
        <table className="shortcuts">
          <tbody>
            {rows.map(([keys, what]) => (
              <tr key={keys}><td><kbd>{keys}</kbd></td><td>{what}</td></tr>
            ))}
          </tbody>
        </table>
        <div style={{ padding: "0.5rem 0.75rem", textAlign: "right" }}>
          <button className="btn" onClick={close}>Tutup</button>
        </div>
      </div>
    </div>
  );
}
