import { useState } from "react";
import { useApp } from "../state/store";

export function NewSessionDialog({ onClose, initialDir }: { onClose: () => void; initialDir?: string | null }) {
  const { providers, sessions, createSession } = useApp();
  const [directory, setDirectory] = useState(initialDir ?? sessions[0]?.project_dir ?? sessions[0]?.directory ?? "");
  const [isolate, setIsolate] = useState(true);
  const [provider, setProvider] = useState(providers?.active_provider ?? "");
  const [model, setModel] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

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
          <input
            data-testid="dir-input"
            value={directory}
            onChange={(e) => setDirectory(e.target.value)}
            placeholder="/path/to/repo (blank = server default)"
          />
        </label>
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
          <input data-testid="model-input" value={model} onChange={(e) => setModel(e.target.value)} />
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
