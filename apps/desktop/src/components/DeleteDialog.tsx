import { useState } from "react";
import { useApp } from "../state/store";

/** Confirms deleting a session; for worktree sessions also asks what to do with the checkout/branch. */
export function DeleteDialog({ sessionId, onClose }: { sessionId: string; onClose: () => void }) {
  const { meta, deleteSession } = useApp();
  const [removeCheckout, setRemoveCheckout] = useState(true);
  const [deleteBranch, setDeleteBranch] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const isolated = !!meta?.branch;

  const confirm = async () => {
    setBusy(true);
    try {
      await deleteSession(sessionId, isolated ? { cleanupWorktree: removeCheckout, deleteBranch } : undefined);
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setBusy(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" data-testid="delete-dialog" onClick={(e) => e.stopPropagation()}>
        <h2>Delete session?</h2>
        <p className="muted">The conversation history is removed permanently. Files in your project are not touched.</p>
        {isolated && (
          <>
            <label className="check">
              <input
                data-testid="delete-checkout"
                type="checkbox"
                checked={removeCheckout}
                onChange={(e) => setRemoveCheckout(e.target.checked)}
              />
              Remove the worktree checkout
            </label>
            <label className="check">
              <input
                data-testid="delete-branch"
                type="checkbox"
                checked={deleteBranch}
                disabled={!removeCheckout}
                onChange={(e) => setDeleteBranch(e.target.checked)}
              />
              Also delete branch <code>{meta?.branch}</code>
              <span className="muted"> — discards commits that were not merged.</span>
            </label>
          </>
        )}
        {error && <div className="notice error">{error}</div>}
        <div className="row end">
          <button onClick={onClose}>Cancel</button>
          <button data-testid="delete-confirm" className="danger" disabled={busy} onClick={() => void confirm()}>
            Delete
          </button>
        </div>
      </div>
    </div>
  );
}
