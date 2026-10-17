import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useApp } from "../state/store";
import type {
  AgentConfig,
  ChatGptStatus,
  McpListResponse,
  PermissionMode,
  PluginManifest,
  ProviderConfig,
  ProvidersListResponse,
  SecuritySettings,
  SkillDefinition,
} from "../api/types";
import type { ProviderKind } from "../api/generated/ProviderKind";

export type Section = "koneksi" | "model" | "izin" | "agent" | "ekstensi" | "tampilan";
const SECTIONS: { id: Section; label: string; hint: string }[] = [
  { id: "koneksi", label: "Koneksi", hint: "Server, notifikasi" },
  { id: "model", label: "Model dan provider", hint: "Provider aktif, kunci API, ChatGPT" },
  { id: "izin", label: "Izin dan keamanan", hint: "Mode izin, sandbox" },
  { id: "agent", label: "Agent", hint: "Batas langkah dan token, memori" },
  { id: "ekstensi", label: "Ekstensi", hint: "MCP, plugin, skill" },
  { id: "tampilan", label: "Tampilan", hint: "Tema" },
];

function errText(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Load something from the server once, with a reload handle. */
function useLoad<T>(load: () => Promise<T>) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const run = useCallback(() => {
    setError(null);
    load().then(setData).catch((e) => setError(errText(e)));
  }, []);
  useEffect(run, [run]);
  return { data, error, reload: run };
}

/** Result line for a save: "Disimpan" or the server's error. */
function useAction() {
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (fn: () => Promise<unknown>, okText = "Disimpan") => {
    setBusy(true);
    setMsg(null);
    try {
      await fn();
      setMsg({ ok: true, text: okText });
    } catch (e) {
      setMsg({ ok: false, text: errText(e) });
    } finally {
      setBusy(false);
    }
  };
  const note = msg && <div className={`notice ${msg.ok ? "" : "error"}`} role="status">{msg.text}</div>;
  return { run, busy, note };
}

function Block({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="sblock">
      <h3 className="strong">{title}</h3>
      {children}
    </section>
  );
}

function Err({ error }: { error: string | null }) {
  return error ? <div className="notice error">{error}</div> : null;
}

// ── Koneksi ──────────────────────────────────────────────────────
function Koneksi() {
  const client = useApp((s) => s.client);
  const connection = useApp((s) => s.connection);
  const notificationsOn = useApp((s) => s.notificationsOn);
  const act = useAction();
  return (
    <>
      <Block title="Server">
        <div className="kv"><span className="muted">Alamat</span><span className="trunc">{client?.baseUrl ?? "-"}</span></div>
        <div className="kv"><span className="muted">Status</span><span>{connection === "online" ? "terhubung" : connection}</span></div>
        <button className="btn deny" style={{ justifySelf: "start" }} onClick={() => useApp.getState().disconnect()}>Putus dari server</button>
      </Block>
      <Block title="Notifikasi">
        <label className="checkrow">
          <input
            type="checkbox" checked={notificationsOn} disabled={act.busy}
            onChange={(e) => void act.run(async () => {
              const granted = await useApp.getState().setNotifications(e.target.checked);
              if (e.target.checked && !granted) throw new Error("Browser tidak memberi izin notifikasi. Aktifkan di pengaturan situs.");
            })}
          />
          <span>Beri tahu saat agent selesai atau butuh izin
            <span className="muted small" style={{ display: "block" }}>Hanya muncul saat tab ini tidak sedang dilihat.</span>
          </span>
        </label>
        {act.note}
      </Block>
    </>
  );
}

// ── Model dan provider ───────────────────────────────────────────
const KIND_LABEL: Record<ProviderKind, string> = {
  anthropic: "Anthropic",
  open_ai_compatible: "Kompatibel OpenAI",
  chat_gpt: "ChatGPT (login)",
};
const emptyProvider = (): ProviderConfig => ({ id: "", name: "", kind: "open_ai_compatible", base_url: "", api_key: null, default_model: null });

function ProviderForm({ initial, isNew, onDone }: { initial: ProviderConfig; isNew: boolean; onDone: () => void }) {
  const client = useApp((s) => s.client);
  const [p, setP] = useState(initial);
  const [key, setKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const act = useAction();
  const set = <K extends keyof ProviderConfig>(k: K, v: ProviderConfig[K]) => setP((x) => ({ ...x, [k]: v }));
  return (
    <form
      className="sform"
      onSubmit={(e) => {
        e.preventDefault();
        void act.run(async () => {
          // null keeps the stored key, "" clears it, anything else replaces it.
          const api_key = clearKey ? "" : key.trim() ? key.trim() : null;
          await client?.upsertProvider({ ...p, api_key, default_model: p.default_model?.trim() || null });
          await useApp.getState().refreshProviders();
          onDone();
        });
      }}
    >
      <label className="label" htmlFor="p-id">ID</label>
      <input id="p-id" className="field" value={p.id} disabled={!isNew} required onChange={(e) => set("id", e.target.value.trim())} autoCapitalize="off" />
      <label className="label" htmlFor="p-name">Nama</label>
      <input id="p-name" className="field" value={p.name} required onChange={(e) => set("name", e.target.value)} />
      <label className="label" htmlFor="p-kind">Jenis</label>
      <select id="p-kind" className="field" value={p.kind} onChange={(e) => set("kind", e.target.value as ProviderKind)}>
        {(Object.keys(KIND_LABEL) as ProviderKind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
      </select>
      {p.kind !== "chat_gpt" && (
        <>
          <label className="label" htmlFor="p-url">Base URL</label>
          <input id="p-url" className="field" value={p.base_url} required inputMode="url" autoCapitalize="off" onChange={(e) => set("base_url", e.target.value)} />
          <label className="label" htmlFor="p-key">Kunci API</label>
          <input id="p-key" className="field" type="password" value={key} autoComplete="off" disabled={clearKey}
            placeholder={isNew ? "" : "Kosongkan untuk mempertahankan kunci yang tersimpan"} onChange={(e) => setKey(e.target.value)} />
          {!isNew && (
            <label className="checkrow"><input type="checkbox" checked={clearKey} onChange={(e) => setClearKey(e.target.checked)} /><span>Hapus kunci yang tersimpan</span></label>
          )}
        </>
      )}
      <label className="label" htmlFor="p-model">Model bawaan (opsional)</label>
      <input id="p-model" className="field" value={p.default_model ?? ""} autoCapitalize="off" onChange={(e) => set("default_model", e.target.value)} />
      {act.note}
      <div style={{ display: "flex", gap: 8 }}>
        <button className="btn send" type="submit" disabled={act.busy}>Simpan provider</button>
        <button className="btn" type="button" onClick={onDone}>Batal</button>
      </div>
    </form>
  );
}

function ChatGpt() {
  const client = useApp((s) => s.client);
  const status = useLoad<ChatGptStatus>(() => client!.chatgptStatus());
  const [attempt, setAttempt] = useState<{ id: string; url: string } | null>(null);
  const act = useAction();

  useEffect(() => {
    if (!attempt || !client) return;
    const t = setInterval(() => {
      client.chatgptLoginStatus(attempt.id).then((r) => {
        if (r.state === "pending") return;
        setAttempt(null);
        status.reload();
      }).catch(() => setAttempt(null));
    }, 2000);
    return () => clearInterval(t);
  }, [attempt, client, status.reload]);

  return (
    <Block title="Akun ChatGPT">
      <Err error={status.error} />
      <div className="kv"><span className="muted">Masuk sebagai</span><span>{status.data?.email ?? "belum masuk"}</span></div>
      {status.data?.accounts.map((a) => (
        <div key={a.client_id} className="kv">
          <span className="trunc">{a.email}{a.active ? " (aktif)" : ""}</span>
          {!a.active && <button className="btn small" onClick={() => void act.run(async () => { await client!.chatgptUseAccount(a.client_id); status.reload(); })}>Pakai</button>}
        </div>
      ))}
      {attempt ? (
        <div className="notice">
          Selesaikan login di <a href={attempt.url} target="_blank" rel="noreferrer">halaman ini</a>. Menunggu konfirmasi…
          <div><button className="btn small" onClick={() => { void client!.chatgptCancelLogin(attempt.id); setAttempt(null); }}>Batalkan</button></div>
        </div>
      ) : (
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="btn" onClick={() => void act.run(async () => {
            const r = await client!.chatgptLogin({ newAccount: Boolean(status.data?.email), openBrowser: false });
            setAttempt({ id: r.attempt_id, url: r.url });
          }, "Login dimulai")}>{status.data?.email ? "Tambah akun" : "Masuk dengan ChatGPT"}</button>
          {status.data?.email && <button className="btn deny" onClick={() => void act.run(async () => { await client!.chatgptLogout(); status.reload(); }, "Keluar")}>Keluar</button>}
        </div>
      )}
      {act.note}
    </Block>
  );
}

function Model() {
  const client = useApp((s) => s.client);
  const providers = useLoad<ProvidersListResponse>(() => client!.providers());
  const [editing, setEditing] = useState<{ cfg: ProviderConfig; isNew: boolean } | null>(null);
  const [confirmDel, setConfirmDel] = useState<string | null>(null);
  const act = useAction();
  const data = providers.data;
  return (
    <>
      <Block title="Provider">
        <Err error={providers.error} />
        {data && <div className="muted small">Aktif: {data.active_provider}, model {data.active_model}</div>}
        {data?.providers.map((p) => (
          <div key={p.id} className="prow">
            <div style={{ minWidth: 0 }}>
              <div className="strong trunc">{p.name}{p.id === data.active_provider ? " (aktif)" : ""}</div>
              <div className="small muted trunc">{KIND_LABEL[p.kind]}{p.kind !== "chat_gpt" ? `, kunci ${p.has_api_key ? "tersimpan" : "belum ada"}` : ""}</div>
            </div>
            <div className="pacts">
              {p.id !== data.active_provider && (
                <button className="btn small" disabled={act.busy} onClick={() => void act.run(async () => { await client!.setActiveProvider(p.id); providers.reload(); await useApp.getState().refreshProviders(); }, "Provider aktif diganti")}>Jadikan aktif</button>
              )}
              <button className="btn small" onClick={() => setEditing({ cfg: { id: p.id, name: p.name, kind: p.kind, base_url: p.base_url, api_key: null, default_model: p.default_model }, isNew: false })}>Ubah</button>
              {confirmDel === p.id ? (
                <button className="btn small deny" onClick={() => void act.run(async () => { setConfirmDel(null); await client!.deleteProvider(p.id); providers.reload(); await useApp.getState().refreshProviders(); }, "Provider dihapus")}>Hapus provider ini</button>
              ) : (
                <button className="btn small quiet" onClick={() => setConfirmDel(p.id)}>Hapus</button>
              )}
            </div>
          </div>
        ))}
        {act.note}
        {editing ? (
          <ProviderForm initial={editing.cfg} isNew={editing.isNew} onDone={() => { setEditing(null); providers.reload(); }} />
        ) : (
          <button className="btn" style={{ justifySelf: "start" }} onClick={() => setEditing({ cfg: emptyProvider(), isNew: true })}>Tambah provider</button>
        )}
      </Block>
      <ChatGpt />
    </>
  );
}

// ── Izin dan keamanan ────────────────────────────────────────────
const MODES: { id: PermissionMode; label: string; desc: string }[] = [
  { id: "ask", label: "Tanya untuk perintah berbahaya", desc: "Bawaan. Perintah aman jalan sendiri." },
  { id: "strict", label: "Tanya untuk semuanya", desc: "Setiap perintah dan operasi butuh persetujuan." },
  { id: "review", label: "Tanya, hasil bisa dirahasiakan", desc: "Seperti ketat, plus opsi menyembunyikan hasil dari model." },
  { id: "trust", label: "Izinkan semuanya", desc: "Tanpa pertanyaan. Pakai hanya di folder yang aman." },
];
const SANDBOX: { id: "off" | "path_jail" | "bubblewrap" | "auto"; label: string }[] = [
  { id: "auto", label: "Otomatis" },
  { id: "bubblewrap", label: "Bubblewrap (terisolasi)" },
  { id: "path_jail", label: "Batasi path" },
  { id: "off", label: "Mati" },
];

function Izin() {
  const client = useApp((s) => s.client);
  const sec = useLoad<SecuritySettings>(() => client!.security());
  const act = useAction();
  const s = sec.data;
  return (
    <>
      <Block title="Mode izin untuk sesi baru">
        <Err error={sec.error} />
        <p className="muted small" style={{ margin: 0 }}>Sesi yang sudah berjalan tidak berubah.</p>
        {MODES.map((m) => (
          <label key={m.id} className="checkrow">
            <input type="radio" name="mode" checked={s?.permission_mode === m.id} disabled={!s || act.busy}
              onChange={() => {
                if (m.id === "trust" && !window.confirm("Mode ini menjalankan semua perintah tanpa bertanya. Lanjutkan?")) return;
                void act.run(async () => { await client!.setDefaultPermissionMode(m.id); sec.reload(); });
              }} />
            <span>{m.label}<span className="muted small" style={{ display: "block" }}>{m.desc}</span></span>
          </label>
        ))}
        {act.note}
      </Block>
      <Block title="Sandbox">
        <select className="field" aria-label="Mode sandbox" value={s?.sandbox_mode ?? ""} disabled={!s || act.busy}
          onChange={(e) => void act.run(async () => { await client!.setSandboxMode(e.target.value as "off"); sec.reload(); })}>
          {s && !SANDBOX.some((x) => x.id === s.sandbox_mode) && <option value={s.sandbox_mode}>{s.sandbox_mode}</option>}
          {SANDBOX.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
        </select>
      </Block>
      <Block title="Hanya dibaca">
        <p className="muted small" style={{ margin: 0 }}>Bagian ini hanya bisa diubah lewat file konfigurasi server.</p>
        <div className="kv"><span className="muted">Batasi akses di luar folder</span><span>{s ? (s.jail_path_traversal ? "ya" : "tidak") : "-"}</span></div>
        <details className="more"><summary>Path eksternal yang diizinkan ({s?.allowed_external_paths.length ?? 0})</summary>
          <pre className="plist">{s?.allowed_external_paths.join("\n") || "kosong"}</pre></details>
        <details className="more"><summary>Perintah bash tanpa tanya ({s?.bash_whitelist.length ?? 0})</summary>
          <pre className="plist">{s?.bash_whitelist.join("\n") || "kosong"}</pre></details>
      </Block>
    </>
  );
}

// ── Agent ────────────────────────────────────────────────────────
function Num({ label, value, onChange, min = 0, step = 1 }: { label: string; value: number; onChange: (n: number) => void; min?: number; step?: number }) {
  return (
    <label className="numrow">
      <span>{label}</span>
      <input className="field" type="number" inputMode="decimal" min={min} step={step} value={value} onChange={(e) => onChange(Number(e.target.value))} />
    </label>
  );
}
function Tog({ label, desc, value, onChange }: { label: string; desc?: string; value: boolean; onChange: (b: boolean) => void }) {
  return (
    <label className="checkrow">
      <input type="checkbox" checked={value} onChange={(e) => onChange(e.target.checked)} />
      <span>{label}{desc && <span className="muted small" style={{ display: "block" }}>{desc}</span>}</span>
    </label>
  );
}

function Agent() {
  const client = useApp((s) => s.client);
  const loaded = useLoad<AgentConfig>(() => client!.agentConfig());
  const [cfg, setCfg] = useState<AgentConfig | null>(null);
  const act = useAction();
  useEffect(() => { if (loaded.data) setCfg(loaded.data); }, [loaded.data]);
  if (!cfg) return <Err error={loaded.error} />;
  const r = cfg.reflection, c = cfg.consolidation;
  return (
    <form className="sform" onSubmit={(e) => {
      e.preventDefault();
      void act.run(async () => {
        const saved = await client!.setAgentConfig({ max_steps: cfg.max_steps, max_tokens: cfg.max_tokens, reflection: r, consolidation: c });
        setCfg(saved);
      });
    }}>
      <Block title="Batas per giliran">
        <Num label="Langkah maksimum" min={1} value={cfg.max_steps} onChange={(n) => setCfg({ ...cfg, max_steps: n })} />
        <Num label="Token maksimum" min={1} value={cfg.max_tokens} onChange={(n) => setCfg({ ...cfg, max_tokens: n })} />
      </Block>
      <Block title="Refleksi setelah giliran">
        <Tog label="Aktifkan refleksi" desc="Agent menilai hasil kerjanya dan mencatat pelajaran." value={r.enabled} onChange={(b) => setCfg({ ...cfg, reflection: { ...r, enabled: b } })} />
        <Num label="Minimal panggilan tool" value={r.min_tool_calls} onChange={(n) => setCfg({ ...cfg, reflection: { ...r, min_tool_calls: n } })} />
        <Tog label="Buat skill dari pelajaran" value={r.skill_creation} onChange={(b) => setCfg({ ...cfg, reflection: { ...r, skill_creation: b } })} />
      </Block>
      <Block title="Konsolidasi memori">
        <Tog label="Aktifkan konsolidasi" desc="Menggabungkan memori yang hampir sama." value={c.enabled} onChange={(b) => setCfg({ ...cfg, consolidation: { ...c, enabled: b } })} />
        <Num label="Minimal jumlah memori" value={c.min_memories} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, min_memories: n } })} />
        <Num label="Ambang kemiripan (0 sampai 1)" step={0.05} value={c.threshold} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, threshold: n } })} />
        <Num label="Ukuran batch" value={c.batch_size} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, batch_size: n } })} />
        <Tog label="Buat abstraksi tingkat tinggi" value={c.abstraction} onChange={(b) => setCfg({ ...cfg, consolidation: { ...c, abstraction: b } })} />
        <Num label="Minimal memori untuk abstraksi" value={c.abstraction_min_memories} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, abstraction_min_memories: n } })} />
      </Block>
      {act.note}
      <button className="btn send" type="submit" disabled={act.busy} style={{ justifySelf: "start" }}>Simpan pengaturan agent</button>
    </form>
  );
}

// ── Ekstensi ─────────────────────────────────────────────────────
function Ekstensi() {
  const client = useApp((s) => s.client);
  const mcp = useLoad<McpListResponse>(() => client!.mcp());
  const plugins = useLoad<PluginManifest[]>(() => client!.plugins());
  const skills = useLoad<SkillDefinition[]>(() => client!.skills());
  const act = useAction();
  return (
    <>
      <Block title="Server MCP">
        <Err error={mcp.error} />
        {mcp.data?.servers.length === 0 && <p className="muted small" style={{ margin: 0 }}>Belum ada server MCP.</p>}
        {mcp.data?.servers.map((m) => (
          <div key={m.name} className="prow">
            <div style={{ minWidth: 0 }}>
              <div className="strong trunc">{m.name}</div>
              <div className="small muted trunc">{m.command}</div>
              {m.error && <div className="small" style={{ color: "var(--red)" }}>{m.error}</div>}
            </div>
            <span className="small muted">{m.status}, {m.tools.length} tool</span>
          </div>
        ))}
      </Block>
      <Block title="Plugin">
        <Err error={plugins.error} />
        {plugins.data?.length === 0 && <p className="muted small" style={{ margin: 0 }}>Belum ada plugin.</p>}
        {plugins.data?.map((p) => (
          <label key={p.name} className="checkrow">
            <input type="checkbox" checked={p.is_enabled} disabled={act.busy}
              onChange={(e) => void act.run(async () => { await (e.target.checked ? client!.enablePlugin(p.name) : client!.disablePlugin(p.name)); plugins.reload(); skills.reload(); mcp.reload(); }, e.target.checked ? "Plugin diaktifkan" : "Plugin dinonaktifkan")} />
            <span>{p.name} <span className="muted small">v{p.version}</span>
              <span className="muted small" style={{ display: "block" }}>{p.description}</span></span>
          </label>
        ))}
        {act.note}
      </Block>
      <Block title="Skill">
        <Err error={skills.error} />
        {skills.data?.map((s) => (
          <div key={s.name} className="prow">
            <div style={{ minWidth: 0 }}>
              <div className="strong trunc">{s.name}</div>
              <div className="small muted">{s.description}</div>
            </div>
            <span className="small muted">{s.is_built_in ? "bawaan" : "kustom"}</span>
          </div>
        ))}
      </Block>
    </>
  );
}

function Tampilan({ theme, onTheme }: { theme: string; onTheme: () => void }) {
  return (
    <Block title="Tema">
      <div className="kv"><span className="muted">Sekarang</span><span>{theme === "system" ? "otomatis (ikut perangkat)" : theme === "light" ? "terang" : "gelap"}</span></div>
      <button className="btn" style={{ justifySelf: "start" }} onClick={onTheme}>Ganti tema</button>
    </Block>
  );
}

export function Settings({ theme, onTheme, section, onSection, onBack }: {
  theme: string;
  onTheme: () => void;
  section: Section | null;
  onSection: (s: Section | null) => void;
  /** Go to the previous page (the section list on narrow screens). */
  onBack: () => void;
}) {
  // Wide screens show the list and a section side by side, so always have one selected there.
  const wide = typeof window !== "undefined" && window.matchMedia("(min-width: 861px)").matches;
  const current: Section | null = section ?? (wide ? "koneksi" : null);
  const meta = SECTIONS.find((s) => s.id === current);

  return (
    <div className="settings">
      <nav className={`slist ${current ? "hide-narrow" : ""}`} aria-label="Bagian pengaturan">
        <h1 className="strong" style={{ fontSize: 20, margin: "0 0 0.5rem" }}>Pengaturan</h1>
        {SECTIONS.map((s) => (
          <button key={s.id} className="sitem" aria-current={s.id === current} onClick={() => onSection(s.id)}>
            <span className="strong">{s.label}</span>
            <span className="small muted">{s.hint}</span>
          </button>
        ))}
      </nav>
      {current && meta && (
        <div className="sbody">
          <button className="btn quiet back-narrow" onClick={onBack}>Kembali</button>
          <h2 className="strong" style={{ fontSize: 17, margin: 0 }}>{meta.label}</h2>
          {current === "koneksi" && <Koneksi />}
          {current === "model" && <Model />}
          {current === "izin" && <Izin />}
          {current === "agent" && <Agent />}
          {current === "ekstensi" && <Ekstensi />}
          {current === "tampilan" && <Tampilan theme={theme} onTheme={onTheme} />}
        </div>
      )}
    </div>
  );
}
