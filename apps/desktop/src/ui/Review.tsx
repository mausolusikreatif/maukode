import { useMemo, useState } from "react";
import { useApp } from "../state/store";
import { parseUnifiedDiff } from "../state/diff";

export function Review({ onClose }: { onClose: () => void }) {
  const review = useApp((s) => s.review);
  const meta = useApp((s) => s.meta);
  const { selectFile, undoLast, commitWorktree, mergeWorktree } = useApp.getState();
  const [message, setMessage] = useState("");
  const parsed = useMemo(() => (review.diff ? parseUnifiedDiff(review.diff.diff) : null), [review.diff]);
  const wt = review.worktree;

  return (
    <aside className="review" aria-label="Perubahan">
      <div className="head">
        <div className="head-title">
          <div className="t">Perubahan</div>
          <div className="small muted">{review.files.length} file diubah agent</div>
        </div>
        <button className="btn" onClick={onClose}>Tutup</button>
      </div>
      <div className="review-body">
        {review.error && <div className="notice error">{review.error}</div>}
        {!review.loading && review.files.length === 0 && !review.error && (
          <p className="muted" style={{ padding: "1rem" }}>Belum ada perubahan file.</p>
        )}
        {review.files.length > 0 && (
          <div className="files">
            {review.files.map((f) => (
              <button key={f.path} className="file" aria-current={f.path === review.selected} onClick={() => void selectFile(f.path)}>
                <span className="trunc" title={f.path}>{f.path}</span>
                <span className="small">
                  {f.binary ? "biner" : (<><span className="add">+{f.insertions ?? 0}</span> <span className="del">-{f.deletions ?? 0}</span></>)}
                </span>
              </button>
            ))}
          </div>
        )}
        {parsed && (
          <div className="diff">
            {parsed.binary && <p className="muted" style={{ padding: "1rem" }}>File biner, diff tidak ditampilkan.</p>}
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
            {review.diff?.truncated && <p className="muted small" style={{ padding: "0.5rem 1rem" }}>Diff dipotong karena terlalu besar.</p>}
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
          <button className="btn" disabled={review.busy} onClick={() => void undoLast()}>Batalkan giliran terakhir</button>
        )}
        {meta?.branch && wt && (
          <>
            <div className="small muted">
              branch {meta.branch.replace(/^maukode\//, "")}: {wt.commits_ahead} commit di depan{wt.dirty ? ", ada perubahan belum di-commit" : ""}
            </div>
            <input className="field" placeholder="Pesan commit (opsional)" value={message} onChange={(e) => setMessage(e.target.value)} />
            <div style={{ display: "flex", gap: 8 }}>
              <button className="btn" style={{ flex: 1 }} disabled={review.busy || !wt.dirty} onClick={() => void commitWorktree(message)}>Commit</button>
              <button className="btn ink" style={{ flex: 1 }} disabled={review.busy || wt.dirty || wt.commits_ahead === 0} onClick={() => void mergeWorktree(message)}>
                Merge ke proyek
              </button>
            </div>
          </>
        )}
      </div>
    </aside>
  );
}
