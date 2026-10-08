import { useEffect, useState } from "react";
import type { AuditResponse } from "../api/types";
import { useApp } from "../state/store";
import { Button } from "./ui/button";
import { Dialog, DialogContent, DialogTitle } from "./ui/dialog";

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
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent
        data-testid="audit-dialog"
        aria-describedby={undefined}
        className="max-h-[85vh] max-w-3xl overflow-y-auto scrollbar-thin"
      >
        <div className="flex items-center justify-between pr-6">
          <DialogTitle>Audit trail</DialogTitle>
          <Button variant="outline" size="sm" onClick={onClose}>
            Close
          </Button>
        </div>
        {error && (
          <div className="notice error rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        {!audit && !error && <p className="text-sm text-muted-foreground">Loading…</p>}
        {r && (
          <>
            <p className="small text-xs text-muted-foreground" data-testid="audit-counts">
              {r.permission_count} approval decision{r.permission_count === 1 ? "" : "s"} · {r.mode_change_count} mode change
              {r.mode_change_count === 1 ? "" : "s"} · {r.compaction_count} compaction{r.compaction_count === 1 ? "" : "s"}
            </p>
            <h4 className="mt-2 text-sm font-semibold">Approval decisions</h4>
            <div className="table-wrap overflow-x-auto rounded-lg border border-border">
              <table data-testid="audit-permissions" className="w-full text-left text-xs">
                <thead className="bg-secondary/50 text-muted-foreground">
                  <tr>
                    <th className="px-2 py-1.5">When</th>
                    <th className="px-2 py-1.5">Turn</th>
                    <th className="px-2 py-1.5">Tool</th>
                    <th className="px-2 py-1.5">Decision</th>
                    <th className="px-2 py-1.5">Mode</th>
                    <th className="px-2 py-1.5">Risk</th>
                    <th className="px-2 py-1.5">Result shown to</th>
                  </tr>
                </thead>
                <tbody>
                  {r.permissions.map((p, i) => (
                    <tr key={i} className="border-t border-border">
                      <td className="px-2 py-1.5">{time(p.created_at)}</td>
                      <td className="px-2 py-1.5">{p.turn_id}</td>
                      <td className="px-2 py-1.5">
                        <code className="rounded bg-secondary px-1 font-mono">{p.tool_name}</code>
                      </td>
                      <td className="px-2 py-1.5" data-testid="audit-decision">
                        {p.decision}
                      </td>
                      <td className="px-2 py-1.5">{p.mode}</td>
                      <td className="px-2 py-1.5">{p.risk_level}</td>
                      <td className="px-2 py-1.5">{p.result_visibility}</td>
                    </tr>
                  ))}
                  {r.permissions.length === 0 && (
                    <tr>
                      <td colSpan={7} className="px-2 py-2 text-muted-foreground">
                        None recorded.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            <h4 className="mt-2 text-sm font-semibold">Approval mode changes</h4>
            <ul className="plain list-disc space-y-1 pl-5 text-sm">
              {r.mode_changes.map((m, i) => (
                <li key={i}>
                  {time(m.created_at)}: {m.previous_mode} → <strong>{m.new_mode}</strong>{" "}
                  <span className="text-muted-foreground">({m.source})</span>
                </li>
              ))}
              {r.mode_changes.length === 0 && <li className="text-muted-foreground">None recorded.</li>}
            </ul>
            <h4 className="mt-2 text-sm font-semibold">Context compactions</h4>
            <ul className="plain list-disc space-y-1 pl-5 text-sm">
              {r.compactions.map((c, i) => (
                <li key={i}>
                  {time(c.created_at)}: {c.original_message_count} → {c.compacted_message_count} messages, {c.original_tokens} →{" "}
                  {c.compacted_tokens} tokens <span className="text-muted-foreground">({c.trigger})</span>
                </li>
              ))}
              {r.compactions.length === 0 && <li className="text-muted-foreground">None recorded.</li>}
            </ul>
          </>
        )}
      </DialogContent>
    </Dialog>
  );
}
