import { useEffect, useState } from "react";
import { useApp } from "../state/store";
import { baseName } from "../state/format";
import { emptyThread } from "../state/thread";
import { Thread } from "./Thread";
import { Dock } from "./Dock";
import { Composer } from "./Composer";

const EXEC_LABEL: Record<string, { text: string; hint: string }> = {
  host: { text: "Host", hint: "Perintah berjalan langsung di mesin ini" },
  process: { text: "Proses terisolasi", hint: "Perintah berjalan di sel lawang (bubblewrap), kernel sama dengan host" },
  sandbox: { text: "Sandbox", hint: "Perintah berjalan di sel lawang dengan kernel gVisor" },
  vm: { text: "VM", hint: "Perintah berjalan di mesin virtual dengan kernel sendiri" },
};

/** Where this session's commands run. Hidden until the server has answered. */
function ExecTag() {
  const label = useApp((s) => s.execution?.label);
  if (!label) return null;
  const info = EXEC_LABEL[label] ?? { text: label, hint: "" };
  return <span className={`exec ${label === "host" ? "" : "isolated"}`} title={info.hint}>{info.text}</span>;
}

function TitleEditor({ id, title }: { id: string; title: string }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState(title);
  const [error, setError] = useState<string | null>(null);
  const save = async () => {
    try {
      await useApp.getState().renameSession(id, value);
      setEditing(false);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };
  if (!editing) {
    return (
      <button className="title-btn t trunc" title="Ubah nama sesi" onClick={() => { setValue(title); setEditing(true); }}>
        {title || "Tanpa judul"}
      </button>
    );
  }
  return (
    <form onSubmit={(e) => { e.preventDefault(); void save(); }} style={{ display: "grid", gap: 2 }}>
      <input
        className="field" style={{ minHeight: "1.9rem" }} autoFocus value={value} aria-label="Nama sesi"
        onChange={(e) => setValue(e.target.value)} onBlur={() => !error && setEditing(false)}
        onKeyDown={(e) => e.key === "Escape" && setEditing(false)}
      />
      {error && <span className="small" style={{ color: "var(--red)" }}>{error}</span>}
    </form>
  );
}

function ModelPicker({ id, provider, model }: { id: string; provider: string | null; model: string | null }) {
  const client = useApp((s) => s.client);
  const [models, setModels] = useState<string[]>([]);
  useEffect(() => {
    if (!client || !provider) return;
    let live = true;
    client.models(provider).then((r) => live && setModels(r.models)).catch(() => undefined);
    return () => { live = false; };
  }, [client, provider]);
  if (!model) return null;
  const options = models.includes(model) ? models : [model, ...models];
  return (
    <select
      className="model-sel" aria-label="Model" value={model}
      onChange={(e) => void useApp.getState().setSessionModel(id, e.target.value, provider ?? undefined)}
    >
      {options.map((m) => <option key={m} value={m}>{m}</option>)}
    </select>
  );
}

export function Session({ onBack, reviewOpen, onToggleReview }: {
  onBack: () => void;
  reviewOpen: boolean;
  onToggleReview: () => void;
}) {
  const activeId = useApp((s) => s.activeId);
  const meta = useApp((s) => s.meta);
  const thread = useApp((s) => (s.activeId ? s.threads[s.activeId] : undefined)) ?? emptyThread();
  const sessions = useApp((s) => s.sessions);
  const reviewFiles = useApp((s) => s.review.files.length);
  const { respond, loadOlder } = useApp.getState();
  const item = sessions.find((s) => s.id === activeId);
  const state = thread.pending.length > 0 ? "hold" : thread.running ? "run" : "";

  if (!activeId) return null;
  return (
    <>
      <header className="head">
        <button className="btn back-narrow" onClick={onBack} aria-label="Kembali ke daftar">Kembali</button>
        <div className="head-title">
          <TitleEditor key={activeId} id={activeId} title={meta?.title || item?.title || ""} />
          <div className="small muted trunc">
            {meta ? baseName(meta.directory) : ""}{meta?.branch ? ` / ${meta.branch.replace(/^maukode\//, "")}` : ""}
            {" "}<ExecTag />
          </div>
        </div>
        <span className={`dot ${state}`} title={state === "hold" ? "menunggu izin" : state === "run" ? "berjalan" : "siap"} />
        <ModelPicker id={activeId} provider={meta?.provider ?? null} model={meta?.model ?? null} />
        <button className="btn" aria-pressed={reviewOpen} onClick={onToggleReview}>
          Perubahan{reviewFiles > 0 ? ` ${reviewFiles}` : ""}
        </button>
      </header>
      <Thread thread={thread} loadedStart={meta?.loadedStart ?? 0} onLoadOlder={() => void loadOlder()} />
      <div className="bottom">
        <Dock pending={thread.pending} onRespond={(id, d) => void respond(id, d)} />
        <Composer thread={thread} />
      </div>
    </>
  );
}
