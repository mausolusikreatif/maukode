import { useEffect } from "react";
import type { PermissionDecision, PermissionRequest } from "../api/types";
import { isEditableTarget } from "../platform";

const KEYS: Record<string, PermissionDecision> = {
  y: "allow_once",
  a: "allow_session",
  p: "allow_private",
  n: "deny",
};

/** The agent is paused and needs an answer: show exactly what it wants to run. */
export function Dock({ pending, onRespond }: {
  pending: PermissionRequest[];
  onRespond: (requestId: string, decision: PermissionDecision) => void;
}) {
  const req = pending[0];
  const requestId = req?.request_id;

  // Same keys as the terminal UI: y / a / p / n (ignored while typing in a field).
  useEffect(() => {
    if (!requestId) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || isEditableTarget(e.target)) return;
      const decision = KEYS[e.key.toLowerCase()];
      if (decision) {
        e.preventDefault();
        onRespond(requestId, decision);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [requestId, onRespond]);

  if (!req) return null;
  const detail = req.command ?? req.path ?? req.description;
  return (
    <section className="dock" role="alertdialog" aria-label="Permintaan izin" aria-live="assertive">
      <div className="dock-top">
        <span className="strong">Agent menunggu izin: {req.tool_name}</span>
        <span className="small muted">
          risiko <span className="risk" data-r={req.risk_level}>{req.risk_level}</span>
          {pending.length > 1 ? `, ${pending.length - 1} lagi antre` : ""}
        </span>
      </div>
      {req.description && req.description !== detail && <div className="muted">{req.description}</div>}
      <pre className="cmd">{detail}</pre>
      <div className="acts">
        <button className="btn deny" onClick={() => onRespond(req.request_id, "deny")}>
          Tolak <kbd>n</kbd>
        </button>
        <button className="btn ink" onClick={() => onRespond(req.request_id, "allow_once")}>
          Izinkan sekali <kbd>y</kbd>
        </button>
      </div>
      <div className="acts2">
        <button className="btn" onClick={() => onRespond(req.request_id, "allow_session")}>
          <span>Izinkan untuk sesi ini <kbd>a</kbd></span>
          <span className="small muted">Tidak bertanya lagi untuk aksi serupa</span>
        </button>
        <button className="btn" onClick={() => onRespond(req.request_id, "allow_private")}>
          <span>Jalankan privat <kbd>p</kbd></span>
          <span className="small muted">Hasil hanya untuk Anda, tidak dikirim ke model</span>
        </button>
      </div>
    </section>
  );
}
