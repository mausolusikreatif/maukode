import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useApp } from "../state/store";
import type {
  AgentConfig,
  ChatGptStatus,
  ExecutionSettings,
  ExecutionStatus,
  McpListResponse,
  PermissionMode,
  PluginManifest,
  ProviderConfig,
  ProvidersListResponse,
  SecuritySettings,
  SkillDefinition,
} from "../api/types";
import type { ProviderKind } from "../api/generated/ProviderKind";

export type Section = "connection" | "model" | "permissions" | "execution" | "agent" | "extensions" | "appearance";
const SECTIONS: { id: Section; label: string; hint: string }[] = [
  { id: "connection", label: "Connection", hint: "Server, notifications" },
  { id: "model", label: "Models and providers", hint: "Active provider, API key, ChatGPT" },
  { id: "permissions", label: "Permissions and security", hint: "Permission mode, sandbox" },
  { id: "execution", label: "Execution", hint: "Host, sandbox, VM" },
  { id: "agent", label: "Agent", hint: "Step and token limits, memory" },
  { id: "extensions", label: "Extensions", hint: "MCP, plugins, skills" },
  { id: "appearance", label: "Appearance", hint: "Theme" },
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

/** Result line for a save: "Saved" or the server's error. */
function useAction() {
  const [msg, setMsg] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const run = async (fn: () => Promise<unknown>, okText = "Saved") => {
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

// ── Connection ──────────────────────────────────────────────────────
function Connection() {
  const client = useApp((s) => s.client);
  const connection = useApp((s) => s.connection);
  const notificationsOn = useApp((s) => s.notificationsOn);
  const act = useAction();
  return (
    <>
      <Block title="Server">
        <div className="kv"><span className="muted">Address</span><span className="trunc">{client?.baseUrl ?? "-"}</span></div>
        <div className="kv"><span className="muted">Status</span><span>{connection === "online" ? "connected" : connection}</span></div>
        <button className="btn deny" style={{ justifySelf: "start" }} onClick={() => useApp.getState().disconnect()}>Disconnect from server</button>
      </Block>
      <Block title="Notifications">
        <label className="checkrow">
          <input
            type="checkbox" checked={notificationsOn} disabled={act.busy}
            onChange={(e) => void act.run(async () => {
              const granted = await useApp.getState().setNotifications(e.target.checked);
              if (e.target.checked && !granted) throw new Error("The browser did not allow notifications. Enable them in the site settings.");
            })}
          />
          <span>Notify when an agent finishes or needs permission
            <span className="muted small" style={{ display: "block" }}>Only shown while this tab is not in focus.</span>
          </span>
        </label>
        {act.note}
      </Block>
    </>
  );
}

// ── Models and providers ───────────────────────────────────────────
const KIND_LABEL: Record<ProviderKind, string> = {
  anthropic: "Anthropic",
  open_ai_compatible: "OpenAI-compatible",
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
      <label className="label" htmlFor="p-name">Name</label>
      <input id="p-name" className="field" value={p.name} required onChange={(e) => set("name", e.target.value)} />
      <label className="label" htmlFor="p-kind">Type</label>
      <select id="p-kind" className="field" value={p.kind} onChange={(e) => set("kind", e.target.value as ProviderKind)}>
        {(Object.keys(KIND_LABEL) as ProviderKind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
      </select>
      {p.kind !== "chat_gpt" && (
        <>
          <label className="label" htmlFor="p-url">Base URL</label>
          <input id="p-url" className="field" value={p.base_url} required inputMode="url" autoCapitalize="off" onChange={(e) => set("base_url", e.target.value)} />
          <label className="label" htmlFor="p-key">API key</label>
          <input id="p-key" className="field" type="password" value={key} autoComplete="off" disabled={clearKey}
            placeholder={isNew ? "" : "Leave empty to keep the stored key"} onChange={(e) => setKey(e.target.value)} />
          {!isNew && (
            <label className="checkrow"><input type="checkbox" checked={clearKey} onChange={(e) => setClearKey(e.target.checked)} /><span>Remove the stored key</span></label>
          )}
        </>
      )}
      <label className="label" htmlFor="p-model">Default model (optional)</label>
      <input id="p-model" className="field" value={p.default_model ?? ""} autoCapitalize="off" onChange={(e) => set("default_model", e.target.value)} />
      {act.note}
      <div style={{ display: "flex", gap: 8 }}>
        <button className="btn send" type="submit" disabled={act.busy}>Save provider</button>
        <button className="btn" type="button" onClick={onDone}>Cancel</button>
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
    <Block title="ChatGPT account">
      <Err error={status.error} />
      <div className="kv"><span className="muted">Signed in as</span><span>{status.data?.email ?? "not signed in"}</span></div>
      {status.data?.accounts.map((a) => (
        <div key={a.client_id} className="kv">
          <span className="trunc">{a.email}{a.active ? " (active)" : ""}</span>
          {!a.active && <button className="btn small" onClick={() => void act.run(async () => { await client!.chatgptUseAccount(a.client_id); status.reload(); })}>Use</button>}
        </div>
      ))}
      {attempt ? (
        <div className="notice">
          Finish signing in on <a href={attempt.url} target="_blank" rel="noreferrer">this page</a>. Waiting for confirmation…
          <div><button className="btn small" onClick={() => { void client!.chatgptCancelLogin(attempt.id); setAttempt(null); }}>Cancel</button></div>
        </div>
      ) : (
        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="btn" onClick={() => void act.run(async () => {
            const r = await client!.chatgptLogin({ newAccount: Boolean(status.data?.email), openBrowser: false });
            setAttempt({ id: r.attempt_id, url: r.url });
          }, "Sign-in started")}>{status.data?.email ? "Add account" : "Sign in with ChatGPT"}</button>
          {status.data?.email && <button className="btn deny" onClick={() => void act.run(async () => { await client!.chatgptLogout(); status.reload(); }, "Signed out")}>Sign out</button>}
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
        {data && <div className="muted small">Active: {data.active_provider}, model {data.active_model}</div>}
        {data?.providers.map((p) => (
          <div key={p.id} className="prow">
            <div style={{ minWidth: 0 }}>
              <div className="strong trunc">{p.name}{p.id === data.active_provider ? " (active)" : ""}</div>
              <div className="small muted">{KIND_LABEL[p.kind]}{p.kind !== "chat_gpt" ? `, key ${p.has_api_key ? "stored" : "not set"}` : ""}</div>
            </div>
            <div className="pacts">
              {p.id !== data.active_provider && (
                <button className="btn small" disabled={act.busy} onClick={() => void act.run(async () => { await client!.setActiveProvider(p.id); providers.reload(); await useApp.getState().refreshProviders(); }, "Active provider changed")}>Make active</button>
              )}
              <button className="btn small" onClick={() => setEditing({ cfg: { id: p.id, name: p.name, kind: p.kind, base_url: p.base_url, api_key: null, default_model: p.default_model }, isNew: false })}>Edit</button>
              {confirmDel === p.id ? (
                <button className="btn small deny" onClick={() => void act.run(async () => { setConfirmDel(null); await client!.deleteProvider(p.id); providers.reload(); await useApp.getState().refreshProviders(); }, "Provider deleted")}>Delete this provider</button>
              ) : (
                <button className="btn small quiet" onClick={() => setConfirmDel(p.id)}>Delete</button>
              )}
            </div>
          </div>
        ))}
        {act.note}
        {editing ? (
          <ProviderForm initial={editing.cfg} isNew={editing.isNew} onDone={() => { setEditing(null); providers.reload(); }} />
        ) : (
          <button className="btn" style={{ justifySelf: "start" }} onClick={() => setEditing({ cfg: emptyProvider(), isNew: true })}>Add provider</button>
        )}
      </Block>
      <ChatGpt />
    </>
  );
}

// ── Permissions and security ────────────────────────────────────────────
const MODES: { id: PermissionMode; label: string; desc: string }[] = [
  { id: "ask", label: "Ask for risky commands", desc: "Default. Safe commands run on their own." },
  { id: "strict", label: "Ask for everything", desc: "Every command and operation needs approval." },
  { id: "review", label: "Ask, results can be kept private", desc: "Like strict, plus an option to hide the result from the model." },
  { id: "trust", label: "Allow everything", desc: "No questions. Use only in a folder you trust." },
];
const SANDBOX: { id: "off" | "path_jail" | "bubblewrap" | "auto"; label: string }[] = [
  { id: "auto", label: "Automatic" },
  { id: "bubblewrap", label: "Bubblewrap (isolated)" },
  { id: "path_jail", label: "Path jail" },
  { id: "off", label: "Off" },
];

function Permissions() {
  const client = useApp((s) => s.client);
  const sec = useLoad<SecuritySettings>(() => client!.security());
  const act = useAction();
  const s = sec.data;
  return (
    <>
      <Block title="Permission mode for new sessions">
        <Err error={sec.error} />
        <p className="muted small" style={{ margin: 0 }}>Sessions that are already running are not affected.</p>
        {MODES.map((m) => (
          <label key={m.id} className="checkrow">
            <input type="radio" name="mode" checked={s?.permission_mode === m.id} disabled={!s || act.busy}
              onChange={() => {
                if (m.id === "trust" && !window.confirm("This mode runs every command without asking. Continue?")) return;
                void act.run(async () => { await client!.setDefaultPermissionMode(m.id); sec.reload(); });
              }} />
            <span>{m.label}<span className="muted small" style={{ display: "block" }}>{m.desc}</span></span>
          </label>
        ))}
        {act.note}
      </Block>
      <Block title="Sandbox">
        <select className="field" aria-label="Sandbox mode" value={s?.sandbox_mode ?? ""} disabled={!s || act.busy}
          onChange={(e) => void act.run(async () => { await client!.setSandboxMode(e.target.value as "off"); sec.reload(); })}>
          {s && !SANDBOX.some((x) => x.id === s.sandbox_mode) && <option value={s.sandbox_mode}>{s.sandbox_mode}</option>}
          {SANDBOX.map((x) => <option key={x.id} value={x.id}>{x.label}</option>)}
        </select>
      </Block>
      <Block title="Read-only">
        <p className="muted small" style={{ margin: 0 }}>These can only be changed in the server configuration file.</p>
        <div className="kv"><span className="muted">Restrict access outside the folder</span><span>{s ? (s.jail_path_traversal ? "yes" : "no") : "-"}</span></div>
        <details className="more"><summary>Allowed external paths ({s?.allowed_external_paths.length ?? 0})</summary>
          <pre className="plist">{s?.allowed_external_paths.join("\n") || "empty"}</pre></details>
        <details className="more"><summary>Bash commands that run without asking ({s?.bash_whitelist.length ?? 0})</summary>
          <pre className="plist">{s?.bash_whitelist.join("\n") || "empty"}</pre></details>
      </Block>
    </>
  );
}

// ── Execution ─────────────────────────────────────────────────────
type Isolation = "process" | "sandbox" | "vm";
const ISOLATION: { id: Isolation; runtime: string; label: string; desc: string }[] = [
  { id: "process", runtime: "bwrap", label: "Isolated process", desc: "Namespaces on the host kernel (bubblewrap). Fast, but it shares the host kernel." },
  { id: "sandbox", runtime: "gvisor", label: "Sandbox", desc: "The gVisor user-space kernel sits between the command and the host." },
  { id: "vm", runtime: "firecracker", label: "VM", desc: "A virtual machine with its own kernel (Firecracker). Needs an image and KVM." },
];

function Execution() {
  const client = useApp((s) => s.client);
  const refreshExecution = useApp((s) => s.refreshExecution);
  const cfg = useLoad<ExecutionSettings>(() => client!.execution());
  const act = useAction();
  const [backend, setBackend] = useState<"host" | "lawang">("host");
  const [url, setUrl] = useState("");
  const [token, setToken] = useState("");
  const [clearToken, setClearToken] = useState(false);
  const [isolation, setIsolation] = useState<Isolation>("process");
  const [image, setImage] = useState("");
  const [memory, setMemory] = useState("");
  const [status, setStatus] = useState<ExecutionStatus | null>(null);
  const [checking, setChecking] = useState(false);

  const check = useCallback(async () => {
    setChecking(true);
    try {
      setStatus(await client!.executionStatus());
    } catch (e) {
      setStatus({ reachable: false, error: errText(e), runtimes: [] });
    } finally {
      setChecking(false);
    }
  }, [client]);

  // Fill the form from what the server has, and look at the daemon when it is the one in use.
  useEffect(() => {
    const d = cfg.data;
    if (!d) return;
    setBackend(d.backend === "lawang" ? "lawang" : "host");
    setUrl(d.lawang.url);
    setIsolation((d.lawang.isolation as Isolation) ?? "process");
    setImage(d.lawang.image ?? "");
    setMemory(d.lawang.memory_mb ? String(d.lawang.memory_mb) : "");
    setToken("");
    setClearToken(false);
    if (d.backend === "lawang") void check();
  }, [cfg.data, check]);

  const d = cfg.data;
  const runtimeOf = (id: Isolation) => status?.runtimes.find((r) => r.id === ISOLATION.find((i) => i.id === id)!.runtime);
  const needsImage = backend === "lawang" && isolation === "vm" && !image.trim();
  const save = () =>
    act.run(async () => {
      await client!.setExecution({
        backend,
        lawang: {
          url: url.trim(),
          token: clearToken ? "" : token ? token : null,
          isolation,
          image: image.trim() || null,
          memory_mb: memory.trim() ? Number(memory) : null,
        },
      });
      await refreshExecution();
      cfg.reload();
    });

  return (
    <>
      <Block title="Where commands run">
        <Err error={cfg.error} />
        <p className="muted small" style={{ margin: 0 }}>
          Applies to bash commands in every session, from the next turn. Project files stay in the project folder.
        </p>
        <label className="checkrow">
          <input type="radio" name="backend" checked={backend === "host"} disabled={!d || act.busy} onChange={() => setBackend("host")} />
          <span>Host<span className="muted small" style={{ display: "block" }}>Directly on this machine, with the sandbox from Permissions and security.</span></span>
        </label>
        <label className="checkrow">
          <input type="radio" name="backend" checked={backend === "lawang"} disabled={!d || act.busy} onChange={() => setBackend("lawang")} />
          <span>Lawang<span className="muted small" style={{ display: "block" }}>Inside an isolated cell owned by the lawang daemon: process, sandbox, or VM.</span></span>
        </label>
      </Block>

      {backend === "lawang" && (
        <>
          <Block title="lawang daemon">
            <label className="numrow"><span>Address</span>
              <input className="field wide" aria-label="lawang daemon address" value={url} spellCheck={false} onChange={(e) => setUrl(e.target.value)} placeholder="http://127.0.0.1:7777" /></label>
            <label className="numrow"><span>Token</span>
              <input className="field wide" type="password" aria-label="lawang daemon token" autoComplete="off" value={token}
                disabled={clearToken}
                placeholder={d?.lawang.token_set ? "stored" : "no token"}
                onChange={(e) => setToken(e.target.value)} /></label>
            {d?.lawang.token_set && !clearToken && (
              <p className="muted small" style={{ margin: 0 }}>Leave empty to use the stored token.</p>
            )}
            {d?.lawang.token_set && (
              <label className="checkrow"><input type="checkbox" checked={clearToken} onChange={(e) => setClearToken(e.target.checked)} />
                <span>Remove the stored token</span></label>
            )}
            <div className="row-actions">
              <button className="btn" disabled={checking || !d} onClick={() => void check()}>{checking ? "Checking..." : "Check daemon"}</button>
            </div>
            {status && status.reachable === false && <div className="notice error">{status.error}</div>}
            {status && status.reachable === true && <div className="notice ok">The daemon answered.</div>}
            <p className="muted small" style={{ margin: 0 }}>The check uses the settings that are already saved.</p>
          </Block>

          <Block title="Isolation">
            {ISOLATION.map((o) => {
              const rt = runtimeOf(o.id);
              const missing = status?.reachable === true && rt && !rt.available;
              return (
                <label key={o.id} className="checkrow">
                  <input type="radio" name="isolation" checked={isolation === o.id} disabled={act.busy || !!missing} onChange={() => setIsolation(o.id)} />
                  <span>
                    {o.label}
                    <span className="muted small" style={{ display: "block" }}>
                      {missing ? `Not available on the daemon machine: ${rt!.reason ?? "unknown reason"}` : o.desc}
                    </span>
                  </span>
                </label>
              );
            })}
            <label className="numrow"><span>Image</span>
              <input className="field wide" aria-label="Cell image" value={image} spellCheck={false}
                onChange={(e) => setImage(e.target.value)} placeholder={isolation === "vm" ? "alpine:3.20 or /path/to/folder" : "optional"} /></label>
            <p className="muted small" style={{ margin: 0 }}>
              An image name (alpine:3.20) that the daemon downloads, or the path of an extracted image folder. {isolation === "vm" ? "A VM requires an image." : "Without an image, the cell uses the host libraries."}
            </p>
            <label className="numrow"><span>Memory (MiB)</span>
              <input className="field" type="number" min={1} inputMode="numeric" aria-label="Cell memory limit" value={memory} onChange={(e) => setMemory(e.target.value)} placeholder="no limit" /></label>
            {needsImage && <div className="notice">A VM needs an image before it can be saved.</div>}
            {isolation === "vm" && (
              <div className="notice">
                In a VM, the project folder is copied into the VM and copied back when needed. Files you create in your editor while a session runs are not deleted,
                but your changes to the same files outside maukode can be overwritten by the version from the VM.
              </div>
            )}
          </Block>
        </>
      )}

      <Block title="Save">
        <button className="btn ink" style={{ justifySelf: "start" }} disabled={!d || act.busy || needsImage} onClick={() => void save()}>Save</button>
        {act.note}
        {d && d.problems.length > 0 && d.backend === "lawang" && <div className="notice error">{d.problems.join("; ")}</div>}
        <p className="muted small" style={{ margin: 0 }}>
          MCP servers still run on the host. If the daemon cannot be reached, commands fail with an explanation and are never run on the host.
        </p>
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
      <Block title="Limits per turn">
        <Num label="Maximum steps" min={1} value={cfg.max_steps} onChange={(n) => setCfg({ ...cfg, max_steps: n })} />
        <Num label="Maximum tokens" min={1} value={cfg.max_tokens} onChange={(n) => setCfg({ ...cfg, max_tokens: n })} />
      </Block>
      <Block title="Reflection after a turn">
        <Tog label="Enable reflection" desc="The agent reviews its work and records lessons." value={r.enabled} onChange={(b) => setCfg({ ...cfg, reflection: { ...r, enabled: b } })} />
        <Num label="Minimum tool calls" value={r.min_tool_calls} onChange={(n) => setCfg({ ...cfg, reflection: { ...r, min_tool_calls: n } })} />
        <Tog label="Create skills from lessons" value={r.skill_creation} onChange={(b) => setCfg({ ...cfg, reflection: { ...r, skill_creation: b } })} />
      </Block>
      <Block title="Memory consolidation">
        <Tog label="Enable consolidation" desc="Merges memories that are nearly the same." value={c.enabled} onChange={(b) => setCfg({ ...cfg, consolidation: { ...c, enabled: b } })} />
        <Num label="Minimum number of memories" value={c.min_memories} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, min_memories: n } })} />
        <Num label="Similarity threshold (0 to 1)" step={0.05} value={c.threshold} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, threshold: n } })} />
        <Num label="Batch size" value={c.batch_size} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, batch_size: n } })} />
        <Tog label="Create high-level abstractions" value={c.abstraction} onChange={(b) => setCfg({ ...cfg, consolidation: { ...c, abstraction: b } })} />
        <Num label="Minimum memories for an abstraction" value={c.abstraction_min_memories} onChange={(n) => setCfg({ ...cfg, consolidation: { ...c, abstraction_min_memories: n } })} />
      </Block>
      {act.note}
      <button className="btn send" type="submit" disabled={act.busy} style={{ justifySelf: "start" }}>Save agent settings</button>
    </form>
  );
}

// ── Extensions ─────────────────────────────────────────────────────
function Extensions() {
  const client = useApp((s) => s.client);
  const mcp = useLoad<McpListResponse>(() => client!.mcp());
  const plugins = useLoad<PluginManifest[]>(() => client!.plugins());
  const skills = useLoad<SkillDefinition[]>(() => client!.skills());
  const act = useAction();
  return (
    <>
      <Block title="MCP servers">
        <Err error={mcp.error} />
        {mcp.data?.servers.length === 0 && <p className="muted small" style={{ margin: 0 }}>No MCP servers yet.</p>}
        {mcp.data?.servers.map((m) => (
          <div key={m.name} className="prow">
            <div style={{ minWidth: 0 }}>
              <div className="strong trunc">{m.name}</div>
              <div className="small muted trunc">{m.command}</div>
              {m.error && <div className="small" style={{ color: "var(--red)" }}>{m.error}</div>}
            </div>
            <span className="small muted">{m.status}, {m.tools.length} tools</span>
          </div>
        ))}
      </Block>
      <Block title="Plugins">
        <Err error={plugins.error} />
        {plugins.data?.length === 0 && <p className="muted small" style={{ margin: 0 }}>No plugins yet.</p>}
        {plugins.data?.map((p) => (
          <label key={p.name} className="checkrow">
            <input type="checkbox" checked={p.is_enabled} disabled={act.busy}
              onChange={(e) => void act.run(async () => { await (e.target.checked ? client!.enablePlugin(p.name) : client!.disablePlugin(p.name)); plugins.reload(); skills.reload(); mcp.reload(); }, e.target.checked ? "Plugin enabled" : "Plugin disabled")} />
            <span>{p.name} <span className="muted small">v{p.version}</span>
              <span className="muted small" style={{ display: "block" }}>{p.description}</span></span>
          </label>
        ))}
        {act.note}
      </Block>
      <Block title="Skills">
        <Err error={skills.error} />
        {skills.data?.map((s) => (
          <div key={s.name} className="prow">
            <div style={{ minWidth: 0 }}>
              <div className="strong trunc">{s.name}</div>
              <div className="small muted">{s.description}</div>
            </div>
            <span className="small muted">{s.is_built_in ? "built-in" : "custom"}</span>
          </div>
        ))}
      </Block>
    </>
  );
}

function Appearance({ theme, onTheme }: { theme: string; onTheme: () => void }) {
  return (
    <Block title="Theme">
      <div className="kv"><span className="muted">Current</span><span>{theme === "system" ? "automatic (follows the device)" : theme === "light" ? "light" : "dark"}</span></div>
      <button className="btn" style={{ justifySelf: "start" }} onClick={onTheme}>Change theme</button>
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
  const current: Section | null = section ?? (wide ? "connection" : null);
  const meta = SECTIONS.find((s) => s.id === current);

  return (
    <>
      <header className="head">
        {current && <button className="btn back-narrow" onClick={onBack}>Back</button>}
        <div className="head-title">
          <div className="t">{current && meta ? meta.label : "Settings"}</div>
        </div>
      </header>
      <div className="settings">
        <nav className={`slist ${current ? "hide-narrow" : ""}`} aria-label="Settings sections">
          {SECTIONS.map((s) => (
            <button key={s.id} className="orow" aria-current={s.id === current} onClick={() => onSection(s.id)}>
              <span className="strong">{s.label}</span>
              <span className="small muted">{s.hint}</span>
            </button>
          ))}
        </nav>
        {current && meta && (
          <div className="sbody">
            {current === "connection" && <Connection />}
            {current === "model" && <Model />}
            {current === "permissions" && <Permissions />}
            {current === "execution" && <Execution />}
            {current === "agent" && <Agent />}
            {current === "extensions" && <Extensions />}
            {current === "appearance" && <Appearance theme={theme} onTheme={onTheme} />}
          </div>
        )}
      </div>
    </>
  );
}
