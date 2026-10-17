import { useMemo, useState } from "react";
import { useApp } from "../state/store";
import { baseName } from "../state/format";
import { loadRecentRepositories, recentRepositories, rememberRepository } from "../state/repositories";

/** New session form: shown when no session is open. */
export function Home() {
  const sessions = useApp((s) => s.sessions);
  const createSession = useApp((s) => s.createSession);
  const recents = useMemo(() => recentRepositories(loadRecentRepositories(), sessions), [sessions]);
  const [dir, setDir] = useState(recents[0] ?? "");
  const [worktree, setWorktree] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      const directory = dir.trim() || null;
      if (directory) rememberRepository(directory);
      await createSession({
        directory, title: null, provider: null, model: null,
        isolation: worktree ? "worktree" : null,
      });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="home">
      <form className="home-in" onSubmit={(e) => { e.preventDefault(); void start(); }}>
        <h1 className="strong" style={{ fontSize: 20, margin: 0 }}>Sesi baru</h1>
        <p className="muted" style={{ margin: 0 }}>
          Pilih folder proyek tempat agent bekerja. Kosongkan untuk memakai folder bawaan server.
        </p>
        <label className="label" htmlFor="dir">Folder proyek</label>
        <input
          id="dir" className="field" value={dir} placeholder="/home/kamu/proyek"
          list="recent-dirs" autoCapitalize="off" autoCorrect="off" spellCheck={false}
          onChange={(e) => setDir(e.target.value)}
        />
        <datalist id="recent-dirs">{recents.map((r) => <option key={r} value={r} />)}</datalist>
        {recents.length > 0 && (
          <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
            {recents.slice(0, 5).map((r) => (
              <button key={r} type="button" className="btn small" title={r} onClick={() => setDir(r)}>{baseName(r)}</button>
            ))}
          </div>
        )}
        <label className="checkrow">
          <input type="checkbox" checked={worktree} onChange={(e) => setWorktree(e.target.checked)} />
          <span>
            Jalankan di git worktree terpisah
            <span className="muted small" style={{ display: "block" }}>
              Agent bekerja di branch sendiri, jadi beberapa agent tidak saling menimpa.
            </span>
          </span>
        </label>
        {error && <div className="notice error">{error}</div>}
        <button className="btn send" type="submit" disabled={busy} style={{ justifySelf: "start" }}>
          {busy ? "Membuat…" : "Mulai sesi"}
        </button>
      </form>
    </div>
  );
}
