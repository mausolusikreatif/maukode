import { useState } from "react";
import { useApp } from "../state/store";
import { Button } from "./ui/button";
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "./ui/dialog";

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
    <Dialog open onOpenChange={(open) => !open && onClose()}>
      <DialogContent data-testid="delete-dialog">
        <DialogTitle>Delete session?</DialogTitle>
        <DialogDescription>
          The conversation history is removed permanently. Files in your project are not touched.
        </DialogDescription>
        {isolated && (
          <div className="space-y-2">
            <label className="check flex items-center gap-2 text-sm">
              <input
                data-testid="delete-checkout"
                type="checkbox"
                className="size-4 accent-[var(--primary)]"
                checked={removeCheckout}
                onChange={(e) => setRemoveCheckout(e.target.checked)}
              />
              Remove the worktree checkout
            </label>
            <label className="check flex items-center gap-2 text-sm">
              <input
                data-testid="delete-branch"
                type="checkbox"
                className="size-4 accent-[var(--primary)]"
                checked={deleteBranch}
                disabled={!removeCheckout}
                onChange={(e) => setDeleteBranch(e.target.checked)}
              />
              <span>
                Also delete branch <code className="rounded bg-secondary px-1 font-mono text-xs">{meta?.branch}</code>
                <span className="text-muted-foreground"> — discards commits that were not merged.</span>
              </span>
            </label>
          </div>
        )}
        {error && (
          <div className="notice error rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        )}
        <div className="row end flex justify-end gap-2">
          <Button variant="outline" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button
            data-testid="delete-confirm"
            variant="destructive"
            size="sm"
            disabled={busy}
            onClick={() => void confirm()}
          >
            Delete
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
