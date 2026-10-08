import { useMemo, useState } from "react";
import type { FileDiffEntry } from "../api/types";
import { openInEditor, revealInFileManager } from "../platform";
import { parseUnifiedDiff } from "../state/diff";
import { useApp, type ActionResult } from "../state/store";
import { cn } from "@/lib/utils";
import { Button } from "./ui/button";
import { useConfirm } from "./ui/confirm";
import { Input } from "./ui/input";

const STATUS_LABEL: Record<FileDiffEntry["status"], string> = { added: "A", modified: "M", deleted: "D" };

function FileRow({ file, active, onSelect }: { file: FileDiffEntry; active: boolean; onSelect: () => void }) {
  return (
    <button
      data-testid="diff-file"
      className={cn(
        "diff-file flex w-full items-center gap-2 px-3 py-1.5 text-left text-xs",
        active ? "active bg-accent" : "hover:bg-accent/50",
      )}
      onClick={onSelect}
      title={file.path}
    >
      <span
        className={cn(
          "status font-mono font-semibold",
          file.status === "added" && "text-success",
          file.status === "modified" && "text-warning",
          file.status === "deleted" && "text-destructive",
          file.status,
        )}
      >
        {STATUS_LABEL[file.status]}
      </span>
      <span className="path flex-1 truncate">{file.path}</span>
      <span className="counts shrink-0 font-mono text-[11px]">
        {file.binary ? (
          "binary"
        ) : (
          <>
            <span className="plus text-success">+{file.insertions ?? 0}</span>{" "}
            <span className="minus text-destructive">−{file.deletions ?? 0}</span>
          </>
        )}
      </span>
    </button>
  );
}

function DiffView({ text, truncated }: { text: string; truncated: boolean }) {
  const parsed = useMemo(() => parseUnifiedDiff(text), [text]);
  if (parsed.binary) return <div className="muted pad p-3 text-sm text-muted-foreground">Binary file: no preview.</div>;
  if (parsed.hunks.length === 0) return <div className="muted pad p-3 text-sm text-muted-foreground">No textual changes.</div>;
  return (
    <div data-testid="diff-view" className="diff-view scrollbar-thin overflow-auto font-mono text-xs">
      {parsed.hunks.map((h, i) => (
        <div key={i} className="hunk">
          <div className="hunk-head bg-secondary/50 px-2 py-1 text-muted-foreground">{h.header}</div>
          {h.lines.map((l, j) => (
            <div
              key={j}
              data-kind={l.kind}
              className={cn(
                "dline flex gap-2 px-2",
                l.kind === "add" && "bg-success/10",
                l.kind === "del" && "bg-destructive/10",
                l.kind,
              )}
            >
              <span className="no w-8 shrink-0 select-none text-right text-muted-foreground">{l.oldNo ?? ""}</span>
              <span className="no w-8 shrink-0 select-none text-right text-muted-foreground">{l.newNo ?? ""}</span>
              <span className="sign w-3 shrink-0 select-none">{l.kind === "add" ? "+" : l.kind === "del" ? "−" : " "}</span>
              <code className="whitespace-pre">{l.text}</code>
            </div>
          ))}
        </div>
      ))}
      {truncated && (
        <div className="notice warn rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-sm text-warning">
          Diff truncated at 1 MiB.
        </div>
      )}
    </div>
  );
}

function Result({ result }: { result: ActionResult }) {
  return (
    <div
      data-testid="review-result"
      className={cn(
        "notice rounded-md border px-3 py-2 text-sm",
        result.level === "ok"
          ? "border-success/40 bg-success/10 text-success"
          : "border-destructive/40 bg-destructive/10 text-destructive",
        result.level === "ok" ? "ok" : result.level,
      )}
    >
      <div>{result.text}</div>
      {result.files && (
        <ul data-testid="conflict-files" className="mt-1 list-disc pl-5">
          {result.files.map((f) => (
            <li key={f}>
              <code className="font-mono">{f}</code>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function WorktreeSection() {
  const { review, meta, commitWorktree, mergeWorktree } = useApp();
  const [message, setMessage] = useState("");
  const wt = review.worktree;
  if (!wt) return null;
  return (
    <section data-testid="worktree" className="worktree mt-3 space-y-2 rounded-lg border border-border bg-card/40 p-3">
      <h4 className="text-sm font-semibold">
        Worktree <code className="rounded bg-secondary px-1 font-mono text-xs">{wt.branch}</code>
      </h4>
      <div className="text-xs text-muted-foreground">
        {wt.dirty ? "Uncommitted changes" : "Clean"} · {wt.commits_ahead} commit{wt.commits_ahead === 1 ? "" : "s"} ahead of{" "}
        <code className="font-mono">{wt.base_commit.slice(0, 8)}</code>
        <br />
        Merges into the branch currently checked out in{" "}
        <code className="font-mono">{meta?.projectDir ?? wt.project_dir}</code>.
      </div>
      <Input
        data-testid="commit-message"
        placeholder="Commit / merge message (optional)"
        value={message}
        onChange={(e) => setMessage(e.target.value)}
      />
      <div className="row flex gap-2">
        <Button
          data-testid="commit-button"
          variant="outline"
          size="sm"
          disabled={review.busy || !wt.dirty}
          onClick={() => void commitWorktree(message)}
        >
          Commit changes
        </Button>
        <Button
          data-testid="merge-button"
          size="sm"
          disabled={review.busy || (!wt.dirty && wt.commits_ahead === 0)}
          onClick={() => void mergeWorktree(message)}
        >
          Merge into project
        </Button>
      </div>
    </section>
  );
}

export function ReviewPanel({ running }: { running: boolean }) {
  const { review, meta, selectFile, refreshReview, undoLast } = useApp();
  const confirm = useConfirm();
  const [editorNotice, setEditorNotice] = useState<string | null>(null);

  const totals = review.files.reduce(
    (t, f) => ({ add: t.add + (f.insertions ?? 0), del: t.del + (f.deletions ?? 0) }),
    { add: 0, del: 0 },
  );

  const selectedPath = useMemo(() => {
    if (!review.selected) return null;
    if (meta?.directory) {
      return `${meta.directory}/${review.selected}`;
    }
    return review.selected;
  }, [meta?.directory, review.selected]);

  const handleOpen = async (editor: "code" | "cursor") => {
    if (!selectedPath) return;
    const res = await openInEditor(selectedPath, editor);
    if (!res.success && res.error) {
      setEditorNotice(res.error);
      setTimeout(() => setEditorNotice(null), 4000);
    }
  };

  const handleReveal = async () => {
    if (!selectedPath) return;
    const res = await revealInFileManager(selectedPath);
    if (!res.success && res.error) {
      setEditorNotice(res.error);
      setTimeout(() => setEditorNotice(null), 4000);
    }
  };

  return (
    <aside
      data-testid="review-panel"
      className="review flex w-96 shrink-0 flex-col border-l border-border bg-card/30"
    >
      <header className="review-head flex items-center gap-2 border-b border-border px-3 py-2 text-sm">
        <strong>Changes</strong>
        <span data-testid="review-summary" className="text-xs text-muted-foreground">
          {review.files.length} file{review.files.length === 1 ? "" : "s"} ·{" "}
          <span className="plus text-success">+{totals.add}</span>{" "}
          <span className="minus text-destructive">−{totals.del}</span>
        </span>
        <button
          className="ml-auto text-xs text-primary hover:underline"
          onClick={() => void refreshReview()}
          disabled={review.loading}
        >
          Refresh
        </button>
      </header>

      {running && (
        <div className="notice warn m-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
          The agent is running: changes may still be incoming. Undo, commit and merge are disabled until it finishes.
        </div>
      )}
      {review.error && (
        <div className="notice error m-2 rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-xs text-destructive">
          {review.error}
        </div>
      )}

      <div className="review-files scrollbar-thin max-h-56 overflow-y-auto py-1">
        {review.files.map((f) => (
          <FileRow key={f.path} file={f} active={f.path === review.selected} onSelect={() => void selectFile(f.path)} />
        ))}
        {!review.loading && review.files.length === 0 && (
          <p data-testid="no-changes" className="muted pad p-3 text-xs text-muted-foreground">
            {review.checkpoints === 0
              ? "No checkpoints yet: changes are tracked from the first turn."
              : "No changes since the first turn."}
          </p>
        )}
      </div>

      <div className="review-diff flex-1 overflow-hidden border-t border-border">
        {review.selected && (
          <div className="diff-toolbar flex items-center gap-2 border-b border-border px-2 py-1" data-testid="diff-toolbar">
            <span className="diff-file-path flex-1 truncate text-xs" title={selectedPath ?? review.selected}>
              {review.selected}
            </span>
            <div className="diff-file-actions flex items-center gap-1">
              <button
                type="button"
                data-testid="file-open-vscode"
                className="rounded border border-border px-1.5 py-0.5 text-[11px] hover:bg-accent"
                title={`Open in VS Code (${selectedPath})`}
                onClick={() => void handleOpen("code")}
              >
                VS Code
              </button>
              <button
                type="button"
                data-testid="file-open-cursor"
                className="rounded border border-border px-1.5 py-0.5 text-[11px] hover:bg-accent"
                title={`Open in Cursor (${selectedPath})`}
                onClick={() => void handleOpen("cursor")}
              >
                Cursor
              </button>
              <button
                type="button"
                data-testid="file-reveal-manager"
                className="rounded border border-border px-1.5 py-0.5 text-[11px] hover:bg-accent"
                title={`Reveal in File Manager (${selectedPath})`}
                onClick={() => void handleReveal()}
              >
                Reveal
              </button>
            </div>
          </div>
        )}
        {editorNotice && (
          <div className="notice warn small pad m-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
            {editorNotice}
          </div>
        )}
        {review.selected && review.diff && <DiffView text={review.diff.diff} truncated={review.diff.truncated} />}
      </div>

      <div className="review-actions space-y-2 border-t border-border p-3">
        <div className="row flex items-center gap-2">
          <Button
            data-testid="undo-button"
            variant="outline"
            size="sm"
            disabled={review.busy || running || review.checkpoints === 0}
            onClick={() => {
              void (async () => {
                const ok = await confirm({
                  title: "Undo the last turn?",
                  description:
                    "The conversation is kept; files changed by the agent are restored to the previous checkpoint.",
                  confirmLabel: "Undo",
                  destructive: true,
                });
                if (ok) void undoLast();
              })();
            }}
          >
            Undo last turn
          </Button>
          <span className="text-xs text-muted-foreground">
            {review.checkpoints} checkpoint{review.checkpoints === 1 ? "" : "s"}
          </span>
        </div>
        <WorktreeSection />
        {review.result && <Result result={review.result} />}
      </div>
    </aside>
  );
}
