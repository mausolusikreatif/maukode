import type { PermissionDecision, PermissionRequest } from "../api/types";

/** The agent is paused and needs an answer: show exactly what it wants to run. */
export function Dock({ pending, onRespond }: {
  pending: PermissionRequest[];
  onRespond: (requestId: string, decision: PermissionDecision) => void;
}) {
  const req = pending[0];
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
        <button className="btn deny" onClick={() => onRespond(req.request_id, "deny")}>Tolak</button>
        <button className="btn ink" onClick={() => onRespond(req.request_id, "allow_once")}>Izinkan sekali</button>
      </div>
      <button className="btn quiet small" style={{ justifySelf: "start" }} onClick={() => onRespond(req.request_id, "allow_session")}>
        Izinkan untuk sisa sesi ini
      </button>
    </section>
  );
}
