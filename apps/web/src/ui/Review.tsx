import { useMemo, useState } from "react";
import { useApp } from "../state/store";
import { parseUnifiedDiff, type DiffHunk } from "../state/diff";

/** The line of the first change in a hunk (in the new file when it has one). */
function firstChangedLine(hunk: DiffHunk): number | undefined {
  const line = hunk.lines.find((l) => l.kind === "add" || l.kind === "del") ?? hunk.lines[0];
  return line?.newNo ?? line?.oldNo;
}

export function Review({ onClose }: { onClose: () => void }) {
  const review = useApp((s) => s.review);
  const meta = useApp((s) => s.meta);
  const { selectFile, undoLast, commitWorktree, mergeWorktree, openInEditor } = useApp.getState();
  const [message, setMessage] = useState("");
  const parsed = useMemo(() => (review.diff ? parseUnifiedDiff(review.diff.diff) : null), [review.diff]);
  const wt = review.worktree;

  return (
    <aside className="review" aria-label="Changes">
      <div className="head">
        <div className="head-title">
          <div className="t">Changes</div>
          <div className="small muted">{review.files.length} file diubah agent</div>
        </div>
        <button className="btn" onClick={onClose}>Close</button>
      </div>
      <div className="review-body">
        {review.error && <div className="notice error">{review.error}</div>}
        {!review.loading && review.files.length === 0 && !review.error && (
          <p className="muted" style={{ padding: "1rem" }}>No file changes yet.</p>
        )}
        {review.files.length > 0 && (
          <div className="files">
            {review.files.map((f) => (
              <button key={f.path} className="file" aria-current={f.path === review.selected} onClick={() => void selectFile(f.path)}>
                <span className="trunc" title={f.path}>{f.path}</span>
                <span className="small">
                  {f.binary ? "binary" : (<><span className="add">+{f.insertions ?? 0}</span> <span className="del">-{f.deletions ?? 0}</span></>)}
                </span>
              </button>
            ))}
          </div>
        )}
        {review.selected && (
          <div style={{ display: "flex", justifyContent: "flex-end", padding: "0.4rem 0.75rem 0" }}>
            <button
              className="btn quiet small" title="Open in the editor on the machine where the daemon runs"
              onClick={() => review.selected && void openInEditor(review.selected, parsed?.hunks[0] ? firstChangedLine(parsed.hunks[0]) : undefined)}
            >
              Open in editor
            </button>
          </div>
        )}
        {parsed && (
          <div className="diff">
            {parsed.binary && <p className="muted" style={{ padding: "1rem" }}>Binary file, diff not shown.</p>}
            {parsed.hunks.map((h, i) => (
              <div key={i}>
                <div className="hunk">{h.header}</div>
                {h.lines.map((l, j) => (
                  <div key={j} className={`dl ${l.kind === "add" ? "add" : l.kind === "del" ? "del" : ""}`}>
                    <span className="n">{l.oldNo ?? ""}</span>
                    <span className="n">{l.newNo ?? ""}</span>
                    <span>{(l.kind === "add" ? "+" : l.kind === "del" ? "-" : " ") + l.text}</span>
                  </div>
                ))}
              </div>
            ))}
            {review.diff?.truncated && <p className="muted small" style={{ padding: "0.5rem 1rem" }}>The diff is truncated because it is too large.</p>}
          </div>
        )}
      </div>
      <div className="review-acts">
        {review.result && (
          <div className={`notice ${review.result.level === "error" ? "error" : ""}`}>
            {review.result.text}
            {review.result.files?.map((f) => <div key={f}>{f}</div>)}
          </div>
        )}
        {review.checkpoints > 0 && (
          <button className="btn" disabled={review.busy} onClick={() => void undoLast()}>Undo the last turn</button>
        )}
        {meta?.branch && wt && (
          <>
            <div className="small muted">
              branch {meta.branch.replace(/^maukode\//, "")}: {wt.commits_ahead} commit(s) ahead{wt.dirty ? ", uncommitted changes" : ""}
            </div>
            <input className="field" placeholder="Commit message (optional)" value={message} onChange={(e) => setMessage(e.target.value)} />
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn" style={{ flex: 1 }} disabled={review.busy || !wt.dirty} onClick={() => void commitWorktree(message)}>Commit</button>
              <button className="btn ink" style={{ flex: 1 }} disabled={review.busy || wt.dirty || wt.commits_ahead === 0} onClick={() => void mergeWorktree(message)}>
                Merge into project
              </button>
            </div>
          </>
        )}
      </div>
    </aside>
  );
}
