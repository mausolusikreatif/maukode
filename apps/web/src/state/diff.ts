/** Parser for the unified diffs produced by `git diff`, for rendering in the review panel. */

export type DiffLineKind = "add" | "del" | "ctx" | "meta";

export interface DiffLine {
  kind: DiffLineKind;
  text: string;
  /** Line number in the old / new file (absent when the line does not exist on that side). */
  oldNo?: number;
  newNo?: number;
}

export interface DiffHunk {
  header: string;
  lines: DiffLine[];
}

export interface ParsedDiff {
  hunks: DiffHunk[];
  binary: boolean;
  /** Lines added / removed across all hunks. */
  additions: number;
  deletions: number;
}

const HUNK = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

export function parseUnifiedDiff(text: string): ParsedDiff {
  const hunks: DiffHunk[] = [];
  let current: DiffHunk | null = null;
  let oldNo = 0;
  let newNo = 0;
  let additions = 0;
  let deletions = 0;
  let binary = false;

  for (const raw of text.split("\n")) {
    const line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    const m = HUNK.exec(line);
    if (m) {
      current = { header: line, lines: [] };
      hunks.push(current);
      oldNo = Number(m[1]);
      newNo = Number(m[2]);
      continue;
    }
    if (!current) {
      // File header lines before the first hunk.
      if (line.startsWith("Binary files ") || line.startsWith("GIT binary patch")) binary = true;
      continue;
    }
    if (line.startsWith("+")) {
      current.lines.push({ kind: "add", text: line.slice(1), newNo: newNo++ });
      additions++;
    } else if (line.startsWith("-")) {
      current.lines.push({ kind: "del", text: line.slice(1), oldNo: oldNo++ });
      deletions++;
    } else if (line.startsWith("\\")) {
      current.lines.push({ kind: "meta", text: line });
    } else if (line.startsWith(" ")) {
      current.lines.push({ kind: "ctx", text: line.slice(1), oldNo: oldNo++, newNo: newNo++ });
    }
    // Anything else (e.g. the trailing empty string after the final newline) is ignored.
  }
  return { hunks, binary, additions, deletions };
}
