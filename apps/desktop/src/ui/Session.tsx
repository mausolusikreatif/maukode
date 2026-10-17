import { useEffect, useState } from "react";
import { useApp } from "../state/store";
import { baseName } from "../state/format";
import { emptyThread } from "../state/thread";
import { Thread } from "./Thread";
import { Dock } from "./Dock";
import { Composer } from "./Composer";

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
          <div className="t trunc">{meta?.title || item?.title || "Tanpa judul"}</div>
          <div className="small muted trunc">
            {meta ? baseName(meta.directory) : ""}{meta?.branch ? ` / ${meta.branch.replace(/^maukode\//, "")}` : ""}
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
