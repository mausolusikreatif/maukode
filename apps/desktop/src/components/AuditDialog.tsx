import { useEffect, useState } from "react";
import type { AuditResponse } from "../api/types";
import { useApp } from "../state/store";

const time = (iso: string) => new Date(iso).toLocaleString();

/** Provenance for one session: every approval decision, mode change and compaction. */
export function AuditDialog({ sessionId, onClose }: { sessionId: string; onClose: () => void }) {
  const client = useApp((s) => s.client);
  const [audit, setAudit] = useState<AuditResponse | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void client
      ?.audit(sessionId)
      .then(setAudit)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, [client, sessionId]);

  const r = audit?.report;
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal wide" data-testid="audit-dialog" onClick={(e) => e.stopPropagation()}>
        <div className="row spread">
          <h2>Audit trail</h2>
          <button onClick={onClose}>Close</button>
        </div>
        {error && <div className="notice error">{error}</div>}
        {!audit && !error && <p className="muted">Loading…</p>}
        {r && (
          <>
            <p className="muted small" data-testid="audit-counts">
              {r.permission_count} approval decision{r.permission_count === 1 ? "" : "s"} · {r.mode_change_count} mode change
              {r.mode_change_count === 1 ? "" : "s"} · {r.compaction_count} compaction{r.compaction_count === 1 ? "" : "s"}
            </p>
            <h4>Approval decisions</h4>
            <div className="table-wrap">
              <table data-testid="audit-permissions">
                <thead>
                  <tr><th>When</th><th>Turn</th><th>Tool</th><th>Decision</th><th>Mode</th><th>Risk</th><th>Result shown to</th></tr>
                </thead>
                <tbody>
                  {r.permissions.map((p, i) => (
                    <tr key={i}>
                      <td>{time(p.created_at)}</td><td>{p.turn_id}</td><td><code>{p.tool_name}</code></td>
                      <td data-testid="audit-decision">{p.decision}</td><td>{p.mode}</td><td>{p.risk_level}</td><td>{p.result_visibility}</td>
                    </tr>
                  ))}
                  {r.permissions.length === 0 && <tr><td colSpan={7} className="muted">None recorded.</td></tr>}
                </tbody>
              </table>
            </div>
            <h4>Approval mode changes</h4>
            <ul className="plain">
              {r.mode_changes.map((m, i) => (
                <li key={i}>{time(m.created_at)}: {m.previous_mode} → <strong>{m.new_mode}</strong> <span className="muted">({m.source})</span></li>
              ))}
              {r.mode_changes.length === 0 && <li className="muted">None recorded.</li>}
            </ul>
            <h4>Context compactions</h4>
            <ul className="plain">
              {r.compactions.map((c, i) => (
                <li key={i}>
                  {time(c.created_at)}: {c.original_message_count} → {c.compacted_message_count} messages, {c.original_tokens} → {c.compacted_tokens} tokens <span className="muted">({c.trigger})</span>
                </li>
              ))}
              {r.compactions.length === 0 && <li className="muted">None recorded.</li>}
            </ul>
          </>
        )}
      </div>
    </div>
  );
}
