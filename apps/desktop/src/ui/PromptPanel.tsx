import { useEffect, useState } from "react";
import { ApiError } from "../api/client";
import type { SystemPromptInfo } from "../api/types";
import { useApp } from "../state/store";
import { promptSummary, shortAgo, shortCount } from "../state/format";

type Load =
  | { kind: "loading" }
  | { kind: "none" }
  | { kind: "error"; message: string }
  | { kind: "ready"; info: SystemPromptInfo };

/** What the model is sent with every request besides the conversation, as the last one had it. */
export function PromptPanel({ id }: { id: string }) {
  const client = useApp((s) => s.client);
  const [load, setLoad] = useState<Load>({ kind: "loading" });

  useEffect(() => {
    if (!client) return;
    let live = true;
    setLoad({ kind: "loading" });
    client
      .systemPrompt(id)
      .then((info) => live && setLoad({ kind: "ready", info }))
      .catch((e) => {
        if (!live) return;
        if (e instanceof ApiError && e.status === 404) setLoad({ kind: "none" });
        else setLoad({ kind: "error", message: e instanceof Error ? e.message : String(e) });
      });
    return () => { live = false; };
  }, [client, id]);

  if (load.kind === "loading") return <section className="prompt-panel small muted">Memuat…</section>;
  if (load.kind === "none") {
    return <section className="prompt-panel small muted">Belum ada permintaan ke model pada sesi ini.</section>;
  }
  if (load.kind === "error") {
    return <section className="prompt-panel small" style={{ color: "var(--red)" }}>{load.message}</section>;
  }

  const { info } = load;
  const summary = promptSummary(info);
  return (
    <section className="prompt-panel" aria-label="System prompt dan skema tool">
      <div className="dock-top">
        <span className="strong">Biaya tetap per permintaan: ≈{shortCount(summary.fixedTokens)} token</span>
        <span className="small muted">dikirim ulang setiap kali</span>
      </div>
      <div className="small muted">
        System prompt ≈{shortCount(info.system_prompt_tokens)} · skema {summary.toolCount} tool ≈{shortCount(info.tool_schema_tokens)}
      </div>
      {summary.biggest.length > 0 && (
        <div className="chips">
          {summary.biggest.map((t) => (
            <span key={t.name} className="chip">{t.name} {shortCount(t.tokens)}</span>
          ))}
        </div>
      )}
      <details className="more">
        <summary>Lihat system prompt</summary>
        <pre className="cmd">{info.system_prompt}</pre>
      </details>
      <div className="small muted">Snapshot {info.hash.slice(0, 8)}, {shortAgo(info.created_at)} lalu. Angka adalah perkiraan.</div>
    </section>
  );
}
