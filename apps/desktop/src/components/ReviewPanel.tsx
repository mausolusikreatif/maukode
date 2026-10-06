import { useMemo, useState } from "react";
import type { FileDiffEntry } from "../api/types";
import { openInEditor, revealInFileManager } from "../platform";
import { parseUnifiedDiff } from "../state/diff";
import { useApp, type ActionResult } from "../state/store";

const STATUS_LABEL: Record<FileDiffEntry["status"], string> = { added: "A", modified: "M", deleted: "D" };

function FileRow({ file, active, onSelect }: { file: FileDiffEntry; active: boolean; onSelect: () => void }) {
  return (
    <button data-testid="diff-file" className={`diff-file ${active ? "active" : ""}`} onClick={onSelect} title={file.path}>
      <span className={`status ${file.status}`}>{STATUS_LABEL[file.status]}</span>
      <span className="path">{file.path}</span>
      <span className="counts">
        {file.binary ? (
          "binary"
        ) : (
          <>
            <span className="plus">+{file.insertions ?? 0}</span> <span className="minus">−{file.deletions ?? 0}</span>
          </>
        )}
      </span>
    </button>
  );
}

function DiffView({ text, truncated }: { text: string; truncated: boolean }) {
  const parsed = useMemo(() => parseUnifiedDiff(text), [text]);
  if (parsed.binary) return <div className="muted pad">Binary file: no preview.</div>;
  if (parsed.hunks.length === 0) return <div className="muted pad">No textual changes.</div>;
  return (
    <div data-testid="diff-view" className="diff-view">
      {parsed.hunks.map((h, i) => (
        <div key={i} className="hunk">
          <div className="hunk-head">{h.header}</div>
          {h.lines.map((l, j) => (
            <div key={j} data-kind={l.kind} className={`dline ${l.kind}`}>
              <span className="no">{l.oldNo ?? ""}</span>
              <span className="no">{l.newNo ?? ""}</span>
              <span className="sign">{l.kind === "add" ? "+" : l.kind === "del" ? "−" : " "}</span>
              <code>{l.text}</code>
            </div>
          ))}
        </div>
      ))}
      {truncated && <div className="notice warn">Diff truncated at 1 MiB.</div>}
    </div>
  );
}

function Result({ result }: { result: ActionResult }) {
  return (
    <div data-testid="review-result" className={`notice ${result.level === "ok" ? "ok" : result.level}`}>
      <div>{result.text}</div>
      {result.files && (
        <ul data-testid="conflict-files">
          {result.files.map((f) => (
            <li key={f}>
              <code>{f}</code>
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
    <section data-testid="worktree" className="worktree">
      <h4>
        Worktree <code>{wt.branch}</code>
      </h4>
      <div className="muted small">
        {wt.dirty ? "Uncommitted changes" : "Clean"} · {wt.commits_ahead} commit{wt.commits_ahead === 1 ? "" : "s"} ahead of{" "}
        <code>{wt.base_commit.slice(0, 8)}</code>
        <br />
        Merges into the branch currently checked out in <code>{meta?.projectDir ?? wt.project_dir}</code>.
      </div>
      <input
        data-testid="commit-message"
        placeholder="Commit / merge message (optional)"
        value={message}
        onChange={(e) => setMessage(e.target.value)}
      />
      <div className="row">
        <button data-testid="commit-button" disabled={review.busy || !wt.dirty} onClick={() => void commitWorktree(message)}>
          Commit changes
        </button>
        <button
          data-testid="merge-button"
          className="primary"
          disabled={review.busy || (!wt.dirty && wt.commits_ahead === 0)}
          onClick={() => void mergeWorktree(message)}
        >
          Merge into project
        </button>
      </div>
    </section>
  );
}

export function ReviewPanel({ running }: { running: boolean }) {
  const { review, meta, selectFile, refreshReview, undoLast } = useApp();
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
    <aside data-testid="review-panel" className="review">
      <header className="review-head">
        <strong>Changes</strong>
        <span data-testid="review-summary" className="muted small">
          {review.files.length} file{review.files.length === 1 ? "" : "s"} · <span className="plus">+{totals.add}</span>{" "}
          <span className="minus">−{totals.del}</span>
        </span>
        <button className="link" onClick={() => void refreshReview()} disabled={review.loading}>
          Refresh
        </button>
      </header>

      {running && <div className="notice warn small">The agent is running: changes may still be incoming. Undo, commit and merge are disabled until it finishes.</div>}
      {review.error && <div className="notice error">{review.error}</div>}

      <div className="review-files">
        {review.files.map((f) => (
          <FileRow key={f.path} file={f} active={f.path === review.selected} onSelect={() => void selectFile(f.path)} />
        ))}
        {!review.loading && review.files.length === 0 && (
          <p data-testid="no-changes" className="muted pad">
            {review.checkpoints === 0 ? "No checkpoints yet: changes are tracked from the first turn." : "No changes since the first turn."}
          </p>
        )}
      </div>

      <div className="review-diff">
        {review.selected && (
          <div className="diff-toolbar" data-testid="diff-toolbar">
            <span className="diff-file-path" title={selectedPath ?? review.selected}>
              {review.selected}
            </span>
            <div className="diff-file-actions">
              <button
                type="button"
                data-testid="file-open-vscode"
                className="btn-sm"
                title={`Open in VS Code (${selectedPath})`}
                onClick={() => void handleOpen("code")}
              >
                VS Code
              </button>
              <button
                type="button"
                data-testid="file-open-cursor"
                className="btn-sm"
                title={`Open in Cursor (${selectedPath})`}
                onClick={() => void handleOpen("cursor")}
              >
                Cursor
              </button>
              <button
                type="button"
                data-testid="file-reveal-manager"
                className="btn-sm"
                title={`Reveal in File Manager (${selectedPath})`}
                onClick={() => void handleReveal()}
              >
                Reveal
              </button>
            </div>
          </div>
        )}
        {editorNotice && <div className="notice warn small pad">{editorNotice}</div>}
        {review.selected && review.diff && <DiffView text={review.diff.diff} truncated={review.diff.truncated} />}
      </div>

      <div className="review-actions">
        <div className="row">
          <button
            data-testid="undo-button"
            disabled={review.busy || running || review.checkpoints === 0}
            onClick={() => {
              if (confirm("Undo the last turn's file changes? The conversation is kept; files are restored.")) void undoLast();
            }}
          >
            Undo last turn
          </button>
          <span className="muted small">{review.checkpoints} checkpoint{review.checkpoints === 1 ? "" : "s"}</span>
        </div>
        <WorktreeSection />
        {review.result && <Result result={review.result} />}
      </div>
    </aside>
  );
}
