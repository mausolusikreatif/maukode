import { useEffect, useMemo, useState } from "react";
import { isDesktop, pickDirectory } from "../platform";
import {
  loadRecentRepositories,
  recentRepositories,
  rememberRepository,
  repositoryName,
} from "../state/repositories";
import { useApp } from "../state/store";

export function NewSessionDialog({ onClose, initialDir }: { onClose: () => void; initialDir?: string | null }) {
  const { client, providers, sessions, createSession } = useApp();
  const [directory, setDirectory] = useState(initialDir ?? sessions[0]?.project_dir ?? sessions[0]?.directory ?? "");
  const [isolate, setIsolate] = useState(true);
  const [provider, setProvider] = useState(providers?.active_provider ?? "");
  const [model, setModel] = useState("");
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const [savedRepositories, setSavedRepositories] = useState(loadRecentRepositories);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const repositories = useMemo(
    () => recentRepositories(savedRepositories, sessions),
    [savedRepositories, sessions],
  );

  const browse = async () => {
    setError(null);
    try {
      const selected = await pickDirectory(directory);
      if (selected) setDirectory(selected);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  useEffect(() => {
    let cancelled = false;
    if (provider && client) {
      void client
        .models(provider)
        .then((r) => {
          if (!cancelled) setAvailableModels(r.models);
        })
        .catch(() => undefined);
    } else {
      setAvailableModels([]);
    }
    return () => {
      cancelled = true;
    };
  }, [client, provider]);

  const submit = async () => {
    setBusy(true);
    setError(null);
    try {
      await createSession({
        directory: directory || null,
        title: null,
        provider: provider || null,
        model: model || null,
        isolation: isolate ? "worktree" : null,
      });
      if (directory.trim()) setSavedRepositories(rememberRepository(directory, savedRepositories));
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <form
        className="modal"
        onClick={(e) => e.stopPropagation()}
        onSubmit={(e) => {
          e.preventDefault();
          void submit();
        }}
      >
        <h2>New session</h2>
        <label>
          Project directory
          <span className="directory-picker">
            <input
              data-testid="dir-input"
              value={directory}
              onChange={(e) => setDirectory(e.target.value)}
              placeholder="/path/to/repo (blank = server default)"
            />
            {isDesktop() && (
              <button type="button" data-testid="browse-directory" onClick={() => void browse()}>
                Browse…
              </button>
            )}
          </span>
        </label>
        {repositories.length > 0 && (
          <div className="recent-repositories" aria-label="Recent repositories">
            <span className="muted">Recent</span>
            <div className="repository-list">
              {repositories.map((path) => (
                <button
                  type="button"
                  className={directory === path ? "selected" : ""}
                  key={path}
                  title={path}
                  onClick={() => setDirectory(path)}
                >
                  {repositoryName(path)}
                </button>
              ))}
            </div>
          </div>
        )}
        <label className="check">
          <input
            data-testid="isolation-checkbox"
            type="checkbox"
            checked={isolate}
            onChange={(e) => setIsolate(e.target.checked)}
          />
          Run in an isolated git worktree
          <span className="muted"> — parallel agents can't overwrite each other. Requires a git repo with a commit.</span>
        </label>
        <label>
          Provider
          <select data-testid="provider-select" value={provider} onChange={(e) => setProvider(e.target.value)}>
            {providers?.providers.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
                {p.has_api_key || p.kind === "chat_gpt" ? "" : " (no key)"}
              </option>
            ))}
          </select>
        </label>
        <label>
          Model <span className="muted">(blank = provider default)</span>
          <input
            data-testid="model-input"
            list="dialog-model-options"
            value={model}
            onChange={(e) => setModel(e.target.value)}
            placeholder={availableModels[0] ? `e.g. ${availableModels[0]}` : "Leave blank for provider default"}
          />
          {availableModels.length > 0 && (
            <datalist id="dialog-model-options">
              {availableModels.map((m) => (
                <option key={m} value={m} />
              ))}
            </datalist>
          )}
        </label>
        {error && <div className="notice error">{error}</div>}
        <div className="row end">
          <button type="button" onClick={onClose}>
            Cancel
          </button>
          <button data-testid="create-session" className="primary" disabled={busy}>
            {busy ? "Creating…" : "Create"}
          </button>
        </div>
      </form>
    </div>
  );
}
