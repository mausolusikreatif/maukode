import { useCallback, useEffect, useRef, useState } from "react";
import type {
  AgentConfig,
  ChatGptLoginStatus,
  ChatGptStatus,
  McpListResponse,
  PermissionMode,
  PluginManifest,
  ProviderKind,
  SecuritySettings,
  SandboxMode,
  SkillDefinition,
} from "../api/types";
import { notificationsSupported } from "../state/notify";
import { useApp } from "../state/store";
import { cn } from "@/lib/utils";
import { Badge } from "./ui/badge";
import { Button } from "./ui/button";
import { useConfirm } from "./ui/confirm";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "./ui/select";
import { Switch } from "./ui/switch";

const MODES: { value: PermissionMode; label: string }[] = [
  { value: "ask", label: "Ask for risky actions" },
  { value: "strict", label: "Ask for everything" },
  { value: "review", label: "Review (private results)" },
  { value: "trust", label: "Trust (no prompts)" },
];
const SANDBOX_MODES: { value: SandboxMode; label: string }[] = [
  { value: "path_jail", label: "Path jail (default)" },
  { value: "auto", label: "Automatic (strongest available)" },
  { value: "bubblewrap", label: "Bubblewrap (Linux)" },
  { value: "off", label: "Off" },
];

const PROVIDER_KINDS: { value: ProviderKind; label: string }[] = [
  { value: "open_ai_compatible", label: "OpenAI-compatible" },
  { value: "anthropic", label: "Anthropic" },
  { value: "chat_gpt", label: "ChatGPT (Sign in with ChatGPT)" },
];

const errText = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Narrow a possibly-null client with a friendly message instead of a bare non-null assertion. */
function requireClient(client: ReturnType<typeof useApp.getState>["client"]) {
  if (!client) throw new Error("Not connected to a MauKode daemon.");
  return client;
}

/* ------------------------------------------------------------------ */
/*  Shared config fetchers                                             */
/* ------------------------------------------------------------------ */

/**
 * Cache a single in-flight security + agent config fetch so multiple sections
 * don't duplicate the same HTTP request on mount.
 */
function useSharedConfig() {
  const client = useApp((s) => s.client);
  const [security, setSecurity] = useState<SecuritySettings | null>(null);
  const [agent, setAgent] = useState<AgentConfig | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    Promise.all([client?.security(), client?.agentConfig()])
      .then(([sec, agt]) => {
        if (cancelled) return;
        if (sec) setSecurity(sec);
        if (agt) setAgent(agt);
      })
      .catch(() => undefined)
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [client]);

  const refreshSecurity = useCallback(async () => {
    const sec = await client?.security();
    if (sec) setSecurity(sec);
  }, [client]);

  return { security, agent, loading, refreshSecurity, setAgent };
}

/* ------------------------------------------------------------------ */
/*  Small reusable building blocks                                     */
/* ------------------------------------------------------------------ */

function Section({ id, title, children }: { id: string; title: string; children: React.ReactNode }) {
  return (
    <section
      data-testid={`settings-${id}`}
      className="settings-section space-y-3 rounded-xl border border-border bg-card/40 p-4"
    >
      <h2 className="text-sm font-semibold tracking-tight">{title}</h2>
      {children}
    </section>
  );
}

function LoadingRow({ label }: { label: string }) {
  return (
    <div className="flex items-center gap-2 text-sm text-muted-foreground">
      <span className="inline-block size-3 animate-spin rounded-full border-2 border-muted-foreground/30 border-t-muted-foreground" />
      {label}
    </div>
  );
}

function Notice({ level, text }: { level: "ok" | "error"; text: string }) {
  return (
    <div
      data-testid="settings-msg"
      className={cn(
        "notice rounded-md border px-3 py-2 text-sm",
        level === "ok"
          ? "border-success/40 bg-success/10 text-success"
          : "border-destructive/40 bg-destructive/10 text-destructive",
        level === "ok" ? "ok" : "error",
      )}
    >
      {text}
    </div>
  );
}

function InlineError({ text }: { text: string }) {
  return (
    <div className="notice error rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
      {text}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/*  General section                                                    */
/* ------------------------------------------------------------------ */

function GeneralSection({ config }: { config: ReturnType<typeof useSharedConfig> }) {
  const { client, notificationsOn, setNotifications } = useApp();
  const { security, agent, loading } = config;
  const [steps, setSteps] = useState("");
  const [tokens, setTokens] = useState("");
  const [reflectionEnabled, setReflectionEnabled] = useState(false);
  const [reflectionSkillCreation, setReflectionSkillCreation] = useState(false);
  const [msg, setMsg] = useState<{ level: "ok" | "error"; text: string } | null>(null);

  // Don't let a late config response clobber what the user already typed.
  const touched = useRef(false);
  useEffect(() => {
    if (!agent || touched.current) return;
    setSteps(String(agent.max_steps));
    setTokens(String(agent.max_tokens));
    setReflectionEnabled(agent.reflection.enabled);
    setReflectionSkillCreation(agent.reflection.skill_creation);
  }, [agent]);

  const saveMode = async (mode: PermissionMode) => {
    setMsg(null);
    try {
      await requireClient(client).setDefaultPermissionMode(mode);
      await config.refreshSecurity();
      setMsg({ level: "ok", text: "Default approval mode saved." });
    } catch (e) {
      setMsg({ level: "error", text: errText(e) });
    }
  };

  const saveSandbox = async (mode: SandboxMode) => {
    setMsg(null);
    try {
      await requireClient(client).setSandboxMode(mode);
      await config.refreshSecurity();
      setMsg({ level: "ok", text: "Sandbox mode saved for new sessions." });
    } catch (e) {
      setMsg({ level: "error", text: errText(e) });
    }
  };

  const saveLimits = async () => {
    setMsg(null);
    touched.current = true;
    try {
      const updated = await requireClient(client).setAgentConfig({
        max_steps: Number(steps),
        max_tokens: Number(tokens),
        reflection: {
          enabled: reflectionEnabled,
          min_tool_calls: agent?.reflection.min_tool_calls ?? 3,
          skill_creation: reflectionSkillCreation,
        },
      });
      config.setAgent(updated);
      setSteps(String(updated.max_steps));
      setTokens(String(updated.max_tokens));
      setReflectionEnabled(updated.reflection.enabled);
      setReflectionSkillCreation(updated.reflection.skill_creation);
      setMsg({ level: "ok", text: "Limits saved." });
    } catch (e) {
      setMsg({ level: "error", text: errText(e) });
    }
  };

  return (
    <Section id="general" title="General">
      {/* Default permission mode */}
      <div className="space-y-1.5">
        <Label>Default approval mode</Label>
        <Select
          value={security?.permission_mode}
          onValueChange={(v) => void saveMode(v as PermissionMode)}
          disabled={loading || !security}
        >
          <SelectTrigger data-testid="default-mode" className="w-64">
            <SelectValue placeholder="Select mode…" />
          </SelectTrigger>
          <SelectContent>
            {MODES.map((m) => (
              <SelectItem key={m.value} value={m.value}>
                {m.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <span className="text-xs text-muted-foreground">Existing sessions keep their own mode.</span>
      </div>
      <div className="space-y-1.5">
        <Label>Command sandbox</Label>
        <Select value={security?.sandbox_mode} onValueChange={(v) => void saveSandbox(v as SandboxMode)} disabled={loading || !security}>
          <SelectTrigger className="w-64"><SelectValue placeholder="Select sandbox…" /></SelectTrigger>
          <SelectContent>{SANDBOX_MODES.map((m) => <SelectItem key={m.value} value={m.value}>{m.label}</SelectItem>)}</SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">Applies to commands in new sessions. Automatic falls back to the path jail when Bubblewrap is unavailable.</p>
      </div>

      {/* Agent limits */}
      <div className="flex flex-wrap items-end gap-3">
        <div className="space-y-1.5">
          <Label htmlFor="max-steps">Max steps per turn</Label>
          <Input
            id="max-steps"
            data-testid="max-steps"
            type="number"
            min={1}
            max={200}
            value={steps}
            disabled={!agent}
            className="w-32"
            onChange={(e) => {
              touched.current = true;
              setSteps(e.target.value);
            }}
          />
          <span className="block text-xs text-muted-foreground">Range: 1–200</span>
        </div>
        <div className="space-y-1.5">
          <Label htmlFor="max-tokens">Max tokens per turn</Label>
          <Input
            id="max-tokens"
            data-testid="max-tokens"
            type="number"
            min={1}
            max={200000}
            value={tokens}
            disabled={!agent}
            className="w-32"
            onChange={(e) => {
              touched.current = true;
              setTokens(e.target.value);
            }}
          />
          <span className="block text-xs text-muted-foreground">Range: 1–200 000</span>
        </div>
        <Button data-testid="save-limits" size="sm" disabled={!agent} onClick={() => void saveLimits()}>
          Save limits
        </Button>
      </div>

      {/* Reflection */}
      <div className="space-y-2">
        <Label>Self-reflection</Label>
        <label className="flex items-center gap-3 text-sm">
          <Switch
            checked={reflectionEnabled}
            onCheckedChange={(v) => {
              touched.current = true;
              setReflectionEnabled(v);
            }}
          />
          <span>Enable post-turn reflection (extract lessons from completed turns)</span>
        </label>
        {reflectionEnabled && (
          <label className="flex items-center gap-3 text-sm pl-7">
            <Switch
              checked={reflectionSkillCreation}
              onCheckedChange={(v) => {
                touched.current = true;
                setReflectionSkillCreation(v);
              }}
            />
            <span>Allow creating learned skills in <code>.maukode/skills/learned/</code></span>
          </label>
        )}
        <p className="text-xs text-muted-foreground">
          When enabled, the agent reviews its performance after each turn and saves
          lessons to memory. Learned skills are discovered on the next session.
        </p>
      </div>

      {/* Notifications */}
      <label className="flex items-center gap-3 text-sm">
        <Switch
          data-testid="notifications-toggle"
          checked={notificationsOn}
          onCheckedChange={(checked) => void setNotifications(checked)}
          disabled={!notificationsSupported()}
        />
        Desktop notifications when an agent needs approval or finishes
        {!notificationsSupported() && <span className="text-muted-foreground"> — not supported here</span>}
      </label>

      {loading && <LoadingRow label="Loading config…" />}
      {msg && <Notice level={msg.level} text={msg.text} />}
    </Section>
  );
}

/* ------------------------------------------------------------------ */
/*  Providers section                                                  */
/* ------------------------------------------------------------------ */

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
      await requireClient(client).upsertProvider({
        id: f.id.trim(),
        name: f.name.trim() || f.id.trim(),
        kind: f.kind,
        base_url: f.base_url.trim(),
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
      className="provider-form mt-2 space-y-2 rounded-lg border border-border bg-background/40 p-3"
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      {isNew && (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="pf-id">Id</Label>
          <Input id="pf-id" data-testid="pf-id" value={f.id} onChange={(e) => set("id", e.target.value)} placeholder="e.g. openrouter" required />
        </div>
      )}
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="pf-name">Display name</Label>
        <Input id="pf-name" data-testid="pf-name" value={f.name} onChange={(e) => set("name", e.target.value)} placeholder="e.g. OpenRouter" />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="pf-kind">Kind</Label>
        <Select
          value={f.kind}
          onValueChange={(v) => set("kind", v as ProviderKind)}
          disabled={!isNew}
        >
          <SelectTrigger id="pf-kind" data-testid="pf-kind" className="w-full">
            <SelectValue placeholder="Select kind…" />
          </SelectTrigger>
          <SelectContent>
            {PROVIDER_KINDS.map((k) => (
              <SelectItem key={k.value} value={k.value}>
                {k.label}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="pf-base-url">Base URL</Label>
        <Input id="pf-base-url" data-testid="pf-base-url" value={f.base_url} onChange={(e) => set("base_url", e.target.value)} required />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="pf-model">Default model</Label>
        <Input id="pf-model" data-testid="pf-model" value={f.default_model} onChange={(e) => set("default_model", e.target.value)} />
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="pf-key">API key {isNew ? "" : "(leave blank to keep)"}</Label>
        <Input
          id="pf-key"
          data-testid="pf-key"
          type="password"
          value={f.api_key}
          onChange={(e) => set("api_key", e.target.value)}
          placeholder={isNew ? "sk-…" : "••••••••"}
        />
      </div>
      {!isNew && (
        <label className="flex items-center gap-3 text-sm">
          <Switch
            checked={f.removeKey}
            onCheckedChange={(checked) => set("removeKey", checked)}
          />
          Remove the stored key
        </label>
      )}
      {error && <InlineError text={error} />}
      <div className="flex justify-end gap-2">
        <Button type="button" variant="outline" size="sm" onClick={onDone}>Cancel</Button>
        <Button data-testid="pf-save" size="sm" type="submit">Save</Button>
      </div>
    </form>
  );
}

function ProvidersSection() {
  const { providers, client, refreshProviders } = useApp();
  const confirm = useConfirm();
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
      <p className="text-xs text-muted-foreground">API keys are stored on the daemon and never sent back to this window.</p>
      {providers?.providers.map((p) => (
        <div key={p.id} data-testid="provider-row" className="provider-row space-y-2 rounded-lg border border-border bg-background/30 p-3">
          <div className="flex flex-wrap items-center gap-2">
            <strong className="text-sm">{p.name}</strong> <code className="rounded bg-secondary px-1.5 py-0.5 font-mono text-xs">{p.id}</code>
            {p.id === providers.active_provider && <Badge variant="primary" className="badge run">default</Badge>}
            <div className="w-full text-xs text-muted-foreground">
              {p.kind === "chat_gpt" ? "ChatGPT plan (see below)" : `${p.base_url} · ${p.has_api_key ? "key stored" : "no key"}`}
              {p.default_model && <> · model {p.default_model}</>}
            </div>
          </div>
          <div className="flex gap-2">
            {p.id !== providers.active_provider && (
              <Button data-testid="make-default" variant="outline" size="sm" onClick={() => void run(() => requireClient(client).setActiveProvider(p.id))}>Make default</Button>
            )}
            {p.kind !== "chat_gpt" && (
              <Button data-testid="edit-provider" variant="outline" size="sm" onClick={() => setEditing(p.id)}>Edit</Button>
            )}
            {p.id !== providers.active_provider && (
              <button
                data-testid="delete-provider"
                className="text-xs text-destructive hover:underline"
                onClick={() => {
                  void (async () => {
                    const ok = await confirm({
                      title: `Remove provider "${p.name}"?`,
                      description: "Its stored API key is deleted with it. This cannot be undone.",
                      confirmLabel: "Remove",
                      destructive: true,
                    });
                    if (ok) void run(() => requireClient(client).deleteProvider(p.id));
                  })();
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
      {msg && <div data-testid="providers-msg"><InlineError text={msg} /></div>}
      {editing === "new" ? (
        <ProviderEditor isNew initial={blankForm()} onDone={() => setEditing(null)} />
      ) : (
        <Button data-testid="add-provider" variant="secondary" size="sm" onClick={() => setEditing("new")}>Add provider</Button>
      )}
    </Section>
  );
}

/* ------------------------------------------------------------------ */
/*  ChatGPT section                                                    */
/* ------------------------------------------------------------------ */

type Login =
  | { phase: "idle" }
  | { phase: "starting" }
  | { phase: "waiting"; attemptId: string; url: string }
  | { phase: "failed"; error: string };

function ChatGptSection() {
  const { client, refreshProviders } = useApp();
  const confirm = useConfirm();
  const [status, setStatus] = useState<ChatGptStatus | null>(null);
  const [login, setLogin] = useState<Login>({ phase: "idle" });
  const [msg, setMsg] = useState<string | null>(null);
  const attempt = useRef<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await requireClient(client).chatgptStatus());
    } catch (e) {
      setMsg(errText(e));
    }
  }, [client]);

  useEffect(() => {
    void refresh();
    return () => {
      if (attempt.current) void client?.chatgptCancelLogin(attempt.current).catch(() => undefined);
    };
  }, [client, refresh]);

  useEffect(() => {
    if (login.phase !== "waiting") return;
    const { attemptId } = login;
    const timer = setInterval(async () => {
      try {
        const s: ChatGptLoginStatus = await requireClient(client).chatgptLoginStatus(attemptId);
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
      const started = await requireClient(client).chatgptLogin({ newAccount, openBrowser: inShell });
      attempt.current = started.attempt_id;
      if (!inShell) window.open(started.url, "_blank", "noopener");
      setLogin({ phase: "waiting", attemptId: started.attempt_id, url: started.url });
    } catch (e) {
      setLogin({ phase: "failed", error: errText(e) });
    }
  };

  const cancel = async () => {
    if (login.phase === "waiting") await requireClient(client).chatgptCancelLogin(login.attemptId).catch(() => undefined);
    attempt.current = null;
    setLogin({ phase: "idle" });
  };

  const signedIn = !!status?.email;
  return (
    <Section id="chatgpt" title="ChatGPT plan">
      <p className="text-xs text-muted-foreground">
        Use your ChatGPT Plus/Pro allowance through “Sign in with ChatGPT”. The browser that approves the sign-in must run on
        the <strong>same computer as the MauKode daemon</strong> (the callback is a local address). Availability is controlled by
        OpenAI; if sign-in is refused, this app may not be enrolled in their program.
      </p>
      <div data-testid="chatgpt-status" className="text-sm">
        {status === null ? (
          <span className="text-muted-foreground">Checking…</span>
        ) : signedIn ? (
          <>Signed in as <strong data-testid="chatgpt-email">{status.email}</strong></>
        ) : (
          <span data-testid="chatgpt-signed-out">Not signed in.</span>
        )}
      </div>
      {status && status.accounts.length > 1 && (
        <ul className="plain space-y-1 text-sm">
          {status.accounts.map((a) => (
            <li key={a.client_id} className="flex items-center gap-2">
              {a.email} {a.active ? <Badge variant="primary" className="badge run">active</Badge> : (
                <button className="text-primary hover:underline" onClick={async () => { await requireClient(client).chatgptUseAccount(a.client_id); await refresh(); }}>Use</button>
              )}
            </li>
          ))}
        </ul>
      )}

      {login.phase === "waiting" && (
        <div data-testid="chatgpt-waiting" className="notice warn space-y-1 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning">
          <div>Waiting for you to approve the sign-in in your browser…</div>
          <div className="text-xs">
            Page didn’t open? <a data-testid="chatgpt-url" className="underline" href={login.url} target="_blank" rel="noopener noreferrer">Open the sign-in page</a>
          </div>
          <Button data-testid="chatgpt-cancel" variant="outline" size="sm" onClick={() => void cancel()}>Cancel</Button>
        </div>
      )}
      {login.phase === "failed" && <div data-testid="chatgpt-error"><InlineError text={`Sign-in failed: ${login.error}`} /></div>}
      {msg && <InlineError text={msg} />}

      <div className="flex items-center gap-2">
        <Button
          data-testid="chatgpt-signin"
          variant={signedIn ? "outline" : "default"}
          size="sm"
          disabled={login.phase === "starting" || login.phase === "waiting"}
          onClick={() => void start(signedIn)}
        >
          {signedIn ? "Add another account" : login.phase === "failed" ? "Try again" : "Sign in with ChatGPT"}
        </Button>
        {signedIn && (
          <button
            data-testid="chatgpt-signout"
            className="text-xs text-destructive hover:underline"
            onClick={async () => {
              const ok = await confirm({
                title: "Sign out of ChatGPT?",
                description: "This revokes the session on the provider side.",
                confirmLabel: "Sign out",
                destructive: true,
              });
              if (!ok) return;
              try {
                await requireClient(client).chatgptLogout();
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

/* ------------------------------------------------------------------ */
/*  Plugins & Skills section                                           */
/* ------------------------------------------------------------------ */

function PluginsSection() {
  const client = useApp((s) => s.client);
  const [plugins, setPlugins] = useState<PluginManifest[] | null>(null);
  const [skills, setSkills] = useState<SkillDefinition[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [p, s] = await Promise.all([requireClient(client).plugins(), requireClient(client).skills()]);
      setPlugins(p);
      setSkills(s);
      setError(null);
    } catch (e) {
      setError(errText(e));
    }
  }, [client]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const togglePlugin = async (name: string, enable: boolean) => {
    try {
      if (enable) await requireClient(client).enablePlugin(name);
      else await requireClient(client).disablePlugin(name);
      await refresh();
    } catch (e) {
      setError(errText(e));
    }
  };

  const pluginCount = plugins?.length ?? 0;
  const skillCount = skills?.length ?? 0;

  return (
    <Section id="plugins" title="Plugins & Skills">
      <p className="text-xs text-muted-foreground">
        Plugins bundle skills, MCP servers, and rules. Skills are reusable prompt-templates the agent can invoke.
        Toggle plugins on or off; skills and MCP servers defined by a disabled plugin are not loaded.
      </p>

      {error && <InlineError text={error} />}

      {plugins === null && skills === null ? (
        <LoadingRow label="Loading plugins…" />
      ) : pluginCount === 0 ? (
        <p className="text-sm text-muted-foreground">
          No plugins configured. Add plugins to <code className="rounded bg-secondary px-1.5 py-0.5 font-mono text-xs">.maukode/plugins/</code> in your project or home directory.
        </p>
      ) : (
        <div className="space-y-2">
          {plugins?.map((p) => (
            <div
              key={p.name}
              data-testid="plugin-row"
              className="space-y-1 rounded-lg border border-border bg-background/30 p-3"
            >
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <strong className="text-sm">{p.name}</strong>
                  <code className="rounded bg-secondary px-1.5 py-0.5 font-mono text-xs">v{p.version}</code>
                  {p.is_enabled ? (
                    <Badge variant="primary" className="badge run">enabled</Badge>
                  ) : (
                    <Badge variant="default" className="badge">disabled</Badge>
                  )}
                </div>
                <Switch
                  data-testid={`plugin-toggle-${p.name}`}
                  checked={p.is_enabled}
                  onCheckedChange={(checked) => void togglePlugin(p.name, checked)}
                />
              </div>
              {p.description && <p className="text-xs text-muted-foreground">{p.description}</p>}
              {p.author && <p className="text-xs text-muted-foreground">by {p.author}</p>}
              {(p.skills.length > 0 || Object.keys(p.mcp_servers).length > 0 || p.rules.length > 0) && (
                <ul className="plain ml-2 space-y-0.5 text-xs text-muted-foreground">
                  {p.skills.length > 0 && <li>{p.skills.length} skill{p.skills.length > 1 ? "s" : ""}</li>}
                  {Object.keys(p.mcp_servers).length > 0 && <li>{Object.keys(p.mcp_servers).length} MCP server{Object.keys(p.mcp_servers).length > 1 ? "s" : ""}</li>}
                  {p.rules.length > 0 && <li>{p.rules.length} rule{p.rules.length > 1 ? "s" : ""}</li>}
                </ul>
              )}
            </div>
          ))}
        </div>
      )}

      {skillCount > 0 && (
        <details className="rounded-lg border border-border bg-background/20 p-3">
          <summary className="cursor-pointer text-sm font-medium">
            {skillCount} skill{skillCount > 1 ? "s" : ""} available
          </summary>
          <ul className="plain mt-2 space-y-1 text-xs text-muted-foreground">
            {skills?.map((s) => (
              <li key={s.name}>
                <strong className="text-foreground">{s.name}</strong>
                {s.is_built_in && <Badge variant="default" className="ml-1 text-[10px]">built-in</Badge>}
                {s.run_as_subagent && <Badge variant="default" className="ml-1 text-[10px]">subagent</Badge>}
                {s.description && <> — {s.description}</>}
              </li>
            ))}
          </ul>
        </details>
      )}
    </Section>
  );
}

/* ------------------------------------------------------------------ */
/*  Read-only MCP & Security section                                   */
/* ------------------------------------------------------------------ */

function ReadOnlySection({ config }: { config: ReturnType<typeof useSharedConfig> }) {
  const client = useApp((s) => s.client);
  const { security } = config;
  const [mcp, setMcp] = useState<McpListResponse | null>(null);
  const [mcpLoading, setMcpLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setMcpLoading(true);
    void client
      ?.mcp()
      .then(setMcp)
      .catch(() => undefined)
      .finally(() => !cancelled && setMcpLoading(false));
    return () => {
      cancelled = true;
    };
  }, [client]);

  return (
    <>
      <Section id="mcp" title="MCP servers">
        <p className="text-xs text-muted-foreground">Read-only. Servers start when an agent first uses them (listing never starts one); they run commands, so they are configured in the config file (<code>mcpServers</code>) or a project’s <code>.maukode/mcp.json</code>.</p>
        {mcpLoading ? (
          <LoadingRow label="Loading MCP servers…" />
        ) : mcp?.servers.length === 0 ? (
          <p className="text-sm text-muted-foreground">None configured.</p>
        ) : (
          mcp?.servers.map((s) => (
            <div key={s.name} className="rounded-lg border border-border bg-background/30 p-3 text-sm">
              <div className="flex items-center gap-2">
                <strong>{s.name}</strong>
                {s.status === "running" ? <Badge variant="primary" className="badge run">running</Badge> : <Badge variant="default" className="badge">{s.status}</Badge>}
              </div>
              <code className="mt-1 block font-mono text-xs text-muted-foreground">{s.command}</code>
            </div>
          ))
        )}
      </Section>
      <Section id="security" title="Security">
        <p className="text-xs text-muted-foreground">Read-only: edit these in the config file and restart the daemon.</p>
        {security ? (
          <ul className="plain space-y-1 text-sm">
            <li>Restrict file tools to the project directory: <strong>{security.jail_path_traversal ? "on" : "off"}</strong></li>
            <li>Extra allowed paths: {security.allowed_external_paths.length ? security.allowed_external_paths.join(", ") : "none"}</li>
            <li>Commands that never ask: {security.bash_whitelist.length ? <code className="font-mono">{security.bash_whitelist.join(", ")}</code> : "none"}</li>
          </ul>
        ) : (
          <LoadingRow label="Loading security config…" />
        )}
      </Section>
    </>
  );
}

/* ------------------------------------------------------------------ */
/*  Connection section                                                 */
/* ------------------------------------------------------------------ */

function ConnectionSection() {
  const { client, disconnect } = useApp();
  return (
    <Section id="connection" title="Connection">
      <p className="text-sm">
        Connected to <code className="rounded bg-secondary px-1.5 py-0.5 font-mono text-xs">{client?.baseUrl}</code>
      </p>
      <Button data-testid="disconnect" variant="outline" size="sm" onClick={() => disconnect()}>Disconnect</Button>
    </Section>
  );
}

/* ------------------------------------------------------------------ */
/*  Page                                                               */
/* ------------------------------------------------------------------ */

export function SettingsPage() {
  const config = useSharedConfig();
  return (
    <main data-testid="settings" className="settings scrollbar-thin flex flex-1 flex-col gap-4 overflow-y-auto px-6 py-5">
      <div className="flex items-center justify-between">
        <h1 className="text-lg font-semibold tracking-tight">Settings</h1>
      </div>
      <GeneralSection config={config} />
      <ProvidersSection />
      <ChatGptSection />
      <PluginsSection />
      <ReadOnlySection config={config} />
      <ConnectionSection />
    </main>
  );
}
