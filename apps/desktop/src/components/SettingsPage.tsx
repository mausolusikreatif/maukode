import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AgentConfig,
  ChatGptLoginStatus,
  ChatGptStatus,
  McpListResponse,
  PermissionMode,
  ProviderKind,
  SecuritySettings,
} from "../api/types";
import { notificationsSupported } from "../state/notify";
import { useApp } from "../state/store";

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "ask", label: "Ask for risky actions" },
  { value: "strict", label: "Ask for everything" },
  { value: "review", label: "Review (private results)" },
  { value: "trust", label: "Trust (no prompts)" },
];

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

function Section({ id, title, children }: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section data-testid={`settings-${id}`} className="settings-section">
      <h2>{title}</h2>
      {children}
    </section>
  );
}

function GeneralSection() {
  const { client, notificationsOn, setNotifications } = useApp();
  const [security, setSecurity] = useState<SecuritySettings | null>(null);
  const [agent, setAgent] = useState<AgentConfig | null>(null);
  const [steps, setSteps] = useState("");
  const [tokens, setTokens] = useState("");
  const [msg, setMsg] = useState<{ level: "ok" | "error"; text: string } | null>(null);

  useEffect(() => {
    void client?.security().then(setSecurity).catch(() => undefined);
    void client
      ?.agentConfig()
      .then((a) => {
        setAgent(a);
        setSteps(String(a.max_steps));
        setTokens(String(a.max_tokens));
      })
      .catch(() => undefined);
  }, [client]);

  const saveLimits = async () => {
    try {
      const saved = await client!.setAgentConfig({ max_steps: Number(steps), max_tokens: Number(tokens) });
      setAgent(saved);
      setMsg({ level: "ok", text: "Limits saved." });
    } catch (e) {
      setMsg({ level: "error", text: errText(e) });
    }
  };

  return (
    <Section id="general" title="General">
      <label>
        Approval mode for new sessions
        <select
          data-testid="default-mode"
          value={security?.permission_mode ?? ""}
          disabled={!security}
          onChange={async (e) => {
            const mode = e.target.value as PermissionMode;
            setSecurity((s) => (s ? { ...s, permission_mode: mode } : s));
            try {
              await client!.setDefaultPermissionMode(mode);
              setMsg({ level: "ok", text: "Default approval mode saved." });
            } catch (err) {
              setMsg({ level: "error", text: errText(err) });
            }
          }}
        >
          {MODES.map((m) => (
            <option key={m.value} value={m.value}>
              {m.label}
            </option>
          ))}
        </select>
        <span className="muted small">Existing sessions keep their own mode.</span>
      </label>

      <div className="row">
        <label>
          Max steps per turn
          <input data-testid="max-steps" type="number" min={1} max={200} value={steps} onChange={(e) => setSteps(e.target.value)} />
        </label>
        <label>
          Max output tokens
          <input data-testid="max-tokens" type="number" min={1} max={200000} value={tokens} onChange={(e) => setTokens(e.target.value)} />
        </label>
        <button
          data-testid="save-limits"
          disabled={!agent || (Number(steps) === agent.max_steps && Number(tokens) === agent.max_tokens)}
          onClick={() => void saveLimits()}
        >
          Save limits
        </button>
      </div>
      <p className="muted small">If your model rejects large outputs, lower the token limit; raise the step limit for long tasks.</p>

      <label className="check">
        <input
          data-testid="notifications-toggle"
          type="checkbox"
          checked={notificationsOn}
          disabled={!notificationsSupported()}
          onChange={async (e) => {
            const ok = await setNotifications(e.target.checked);
            if (e.target.checked && !ok) setMsg({ level: "error", text: "Notifications were not allowed by the system." });
          }}
        />
        Desktop notifications when an agent needs approval or finishes
        {!notificationsSupported() && <span className="muted"> — not supported here</span>}
      </label>
      {msg && <div data-testid="settings-msg" className={`notice ${msg.level === "ok" ? "ok" : "error"}`}>{msg.text}</div>}
    </Section>
  );
}

interface ProviderForm {
  id: string;
  name: string;
  kind: ProviderKind;
  base_url: string;
  default_model: string;
  api_key: string;
  removeKey: boolean;
}

const blankForm = (): ProviderForm => ({
  id: "", name: "", kind: "open_ai_compatible", base_url: "", default_model: "", api_key: "", removeKey: false,
});

function ProviderEditor({
  initial, isNew, onDone,
}: { initial: ProviderForm; isNew: boolean; onDone: () => void }) {
  const { client, refreshProviders } = useApp();
  const [f, setF] = useState(initial);
  const [error, setError] = useState<string | null>(null);
  const set = <K extends keyof ProviderForm>(k: K, v: ProviderForm[K]) => setF((x) => ({ ...x, [k]: v }));

  const save = async () => {
    try {
      await client!.upsertProvider({
        id: f.id.trim(),
        name: f.name.trim() || f.id.trim(),
        kind: f.kind,
        base_url: f.base_url.trim(),
        // null keeps the stored key, "" removes it, anything else replaces it.
        api_key: f.removeKey ? "" : f.api_key || null,
        default_model: f.default_model.trim() || null,
      });
      await refreshProviders();
      onDone();
    } catch (e) {
      setError(errText(e));
    }
  };

  return (
    <form
      data-testid="provider-form"
      className="provider-form"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {isNew && (
        <label>
          Id <input data-testid="pf-id" value={f.id} onChange={(e) => set("id", e.target.value)} placeholder="e.g. openrouter" required />
        </label>
      )}
      <label>
        Name <input data-testid="pf-name" value={f.name} onChange={(e) => set("name", e.target.value)} />
      </label>
      <label>
        Kind
        <select data-testid="pf-kind" value={f.kind} disabled={!isNew} onChange={(e) => set("kind", e.target.value as ProviderKind)}>
          <option value="open_ai_compatible">OpenAI-compatible</option>
          <option value="anthropic">Anthropic</option>
        </select>
      </label>
      <label>
        Base URL <input data-testid="pf-base-url" value={f.base_url} onChange={(e) => set("base_url", e.target.value)} required />
      </label>
      <label>
        Default model <input data-testid="pf-model" value={f.default_model} onChange={(e) => set("default_model", e.target.value)} />
      </label>
      <label>
        API key <span className="muted">{isNew ? "" : "(leave blank to keep the stored key)"}</span>
        <input data-testid="pf-key" type="password" autoComplete="off" value={f.api_key} onChange={(e) => set("api_key", e.target.value)} />
      </label>
      {!isNew && (
        <label className="check">
          <input type="checkbox" checked={f.removeKey} onChange={(e) => set("removeKey", e.target.checked)} /> Remove the stored key
        </label>
      )}
      {error && <div className="notice error">{error}</div>}
      <div className="row end">
        <button type="button" onClick={onDone}>Cancel</button>
        <button data-testid="pf-save" className="primary">Save</button>
      </div>
    </form>
  );
}

function ProvidersSection() {
  const { providers, client, refreshProviders } = useApp();
  const [editing, setEditing] = useState<string | "new" | null>(null);
  const [msg, setMsg] = useState<string | null>(null);

  const run = async (fn: () => Promise<unknown>) => {
    setMsg(null);
    try {
      await fn();
      await refreshProviders();
    } catch (e) {
      setMsg(errText(e));
    }
  };

  return (
    <Section id="providers" title="Providers">
      <p className="muted small">API keys are stored on the daemon and never sent back to this window.</p>
      {providers?.providers.map((p) => (
        <div key={p.id} data-testid="provider-row" className="provider-row">
          <div>
            <strong>{p.name}</strong> <code>{p.id}</code>
            {p.id === providers.active_provider && <span className="badge run">default</span>}
            <div className="muted small">
              {p.kind === "chat_gpt" ? "ChatGPT plan (see below)" : `${p.base_url} · ${p.has_api_key ? "key stored" : "no key"}`}
              {p.default_model && <> · model {p.default_model}</>}
            </div>
          </div>
          <div className="row">
            {p.id !== providers.active_provider && (
              <button data-testid="make-default" onClick={() => void run(() => client!.setActiveProvider(p.id))}>Make default</button>
            )}
            {p.kind !== "chat_gpt" && (
              <button data-testid="edit-provider" onClick={() => setEditing(p.id)}>Edit</button>
            )}
            {p.id !== providers.active_provider && (
              <button
                data-testid="delete-provider"
                className="link"
                onClick={() => {
                  if (confirm(`Remove provider “${p.name}”?`)) void run(() => client!.deleteProvider(p.id));
                }}
              >
                Remove
              </button>
            )}
          </div>
          {editing === p.id && (
            <ProviderEditor
              isNew={false}
              initial={{ ...blankForm(), id: p.id, name: p.name, kind: p.kind, base_url: p.base_url, default_model: p.default_model ?? "" }}
              onDone={() => setEditing(null)}
            />
          )}
        </div>
      ))}
      {msg && <div data-testid="providers-msg" className="notice error">{msg}</div>}
      {editing === "new" ? (
        <ProviderEditor isNew initial={blankForm()} onDone={() => setEditing(null)} />
      ) : (
        <button data-testid="add-provider" onClick={() => setEditing("new")}>Add provider</button>
      )}
    </Section>
  );
}

type Login =
  | { phase: "idle" }
  | { phase: "starting" }
  | { phase: "waiting"; attemptId: string; url: string }
  | { phase: "failed"; error: string };

function ChatGptSection() {
  const { client, refreshProviders } = useApp();
  const [status, setStatus] = useState<ChatGptStatus | null>(null);
  const [login, setLogin] = useState<Login>({ phase: "idle" });
  const [msg, setMsg] = useState<string | null>(null);
  const attempt = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await client!.chatgptStatus());
    } catch (e) {
      setMsg(errText(e));
    }
  }, [client]);

  useEffect(() => {
    void refresh();
    return () => {
      // Leaving the page abandons an unfinished sign-in so its callback listener is closed.
      if (attempt.current) void client?.chatgptCancelLogin(attempt.current).catch(() => undefined);
    };
  }, [client, refresh]);

  // Poll the daemon while the user approves the sign-in in their browser.
  useEffect(() => {
    if (login.phase !== "waiting") return;
    const { attemptId } = login;
    const timer = setInterval(async () => {
      try {
        const s: ChatGptLoginStatus = await client!.chatgptLoginStatus(attemptId);
        if (s.state === "pending") return;
        attempt.current = null;
        if (s.state === "succeeded") {
          setLogin({ phase: "idle" });
          await refresh();
          await refreshProviders();
        } else {
          setLogin({ phase: "failed", error: s.error });
        }
      } catch (e) {
        attempt.current = null;
        setLogin({ phase: "failed", error: errText(e) });
      }
    }, 1000);
    return () => clearInterval(timer);
  }, [login, client, refresh, refreshProviders]);

  const start = async (newAccount: boolean) => {
    setLogin({ phase: "starting" });
    setMsg(null);
    try {
      const inShell = !!window.__TAURI_INTERNALS__;
      const started = await client!.chatgptLogin({ newAccount, openBrowser: inShell });
      attempt.current = started.attempt_id;
      if (!inShell) window.open(started.url, "_blank", "noopener");
      setLogin({ phase: "waiting", attemptId: started.attempt_id, url: started.url });
    } catch (e) {
      setLogin({ phase: "failed", error: errText(e) });
    }
  };

  const cancel = async () => {
    if (login.phase === "waiting") await client!.chatgptCancelLogin(login.attemptId).catch(() => undefined);
    attempt.current = null;
    setLogin({ phase: "idle" });
  };

  const signedIn = !!status?.email;
  return (
    <Section id="chatgpt" title="ChatGPT plan">
      <p className="muted small">
        Use your ChatGPT Plus/Pro allowance through “Sign in with ChatGPT”. The browser that approves the sign-in must run on
        the <strong>same computer as the MauKode daemon</strong> (the callback is a local address). Availability is controlled by
        OpenAI; if sign-in is refused, this app may not be enrolled in their program.
      </p>
      <div data-testid="chatgpt-status">
        {status === null ? (
          <span className="muted">Checking…</span>
        ) : signedIn ? (
          <>Signed in as <strong data-testid="chatgpt-email">{status.email}</strong></>
        ) : (
          <span data-testid="chatgpt-signed-out">Not signed in.</span>
        )}
      </div>
      {status && status.accounts.length > 1 && (
        <ul className="plain">
          {status.accounts.map((a) => (
            <li key={a.client_id}>
              {a.email} {a.active ? <span className="badge run">active</span> : (
                <button className="link" onClick={async () => { await client!.chatgptUseAccount(a.client_id); await refresh(); }}>Use</button>
              )}
            </li>
          ))}
        </ul>
      )}

      {login.phase === "waiting" && (
        <div data-testid="chatgpt-waiting" className="notice warn">
          <div>Waiting for you to approve the sign-in in your browser…</div>
          <div className="small">
            Page didn’t open? <a data-testid="chatgpt-url" href={login.url} target="_blank" rel="noopener noreferrer">Open the sign-in page</a>
          </div>
          <button data-testid="chatgpt-cancel" onClick={() => void cancel()}>Cancel</button>
        </div>
      )}
      {login.phase === "failed" && <div data-testid="chatgpt-error" className="notice error">Sign-in failed: {login.error}</div>}
      {msg && <div className="notice error">{msg}</div>}

      <div className="row">
        <button
          data-testid="chatgpt-signin"
          className={signedIn ? "" : "primary"}
          disabled={login.phase === "starting" || login.phase === "waiting"}
          onClick={() => void start(signedIn)}
        >
          {signedIn ? "Add another account" : login.phase === "failed" ? "Try again" : "Sign in with ChatGPT"}
        </button>
        {signedIn && (
          <button
            data-testid="chatgpt-signout"
            className="link"
            onClick={async () => {
              if (!confirm("Sign out of ChatGPT? This revokes the session on OpenAI’s side.")) return;
              try {
                await client!.chatgptLogout();
                await refresh();
              } catch (e) {
                setMsg(errText(e));
              }
            }}
          >
            Sign out
          </button>
        )}
      </div>
    </Section>
  );
}

function ReadOnlySection() {
  const client = useApp((s) => s.client);
  const [mcp, setMcp] = useState<McpListResponse | null>(null);
  const [security, setSecurity] = useState<SecuritySettings | null>(null);
  useEffect(() => {
    void client?.mcp().then(setMcp).catch(() => undefined);
    void client?.security().then(setSecurity).catch(() => undefined);
  }, [client]);

  return (
    <>
      <Section id="mcp" title="MCP servers">
        <p className="muted small">Read-only: servers run commands, so they are configured in the config file (<code>mcpServers</code>) or a project’s <code>.maukode/mcp.json</code>.</p>
        {mcp?.servers.length === 0 && <p className="muted">None configured.</p>}
        {mcp?.servers.map((s) => (
          <div key={s.name} className="provider-row">
            <div>
              <strong>{s.name}</strong> <code>{s.command}</code> <span className="badge">{s.status}</span>
              <div className="muted small">{s.tools.length} tool{s.tools.length === 1 ? "" : "s"}{s.error && <> · {s.error}</>}</div>
            </div>
          </div>
        ))}
      </Section>
      <Section id="security" title="Security">
        <p className="muted small">Read-only: edit these in the config file and restart the daemon.</p>
        {security && (
          <ul className="plain">
            <li>Restrict file tools to the project directory: <strong>{security.jail_path_traversal ? "on" : "off"}</strong></li>
            <li>Extra allowed paths: {security.allowed_external_paths.length ? security.allowed_external_paths.join(", ") : "none"}</li>
            <li>Commands that never ask: {security.bash_whitelist.length ? <code>{security.bash_whitelist.join(", ")}</code> : "none"}</li>
          </ul>
        )}
      </Section>
    </>
  );
}

function ConnectionSection() {
  const { client, disconnect } = useApp();
  return (
    <Section id="connection" title="Connection">
      <p>
        Connected to <code>{client?.baseUrl}</code>
      </p>
      <button data-testid="disconnect" onClick={() => disconnect()}>Disconnect</button>
    </Section>
  );
}

export function SettingsPage() {
  return (
    <main data-testid="settings" className="settings">
      <h1>Settings</h1>
      <GeneralSection />
      <ProvidersSection />
      <ChatGptSection />
      <ReadOnlySection />
      <ConnectionSection />
    </main>
  );
}
