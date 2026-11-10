import { describe, expect, it } from "vitest";
import { parseUnifiedDiff } from "./diff";

const SAMPLE = `diff --git a/a.txt b/a.txt
index 111..222 100644
--- a/a.txt
+++ b/a.txt
@@ -1,3 +1,4 @@ fn main
 one
-two
+TWO
+three
 four
\\ No newline at end of file
@@ -10 +11 @@
-x
+y
`;

describe("parseUnifiedDiff", () => {
  it("numbers lines on both sides and counts changes", () => {
    const d = parseUnifiedDiff(SAMPLE);
    expect(d.hunks).toHaveLength(2);
    expect(d.additions).toBe(3);
    expect(d.deletions).toBe(2);
    const [first] = d.hunks;
    expect(first.header).toBe("@@ -1,3 +1,4 @@ fn main");
    expect(first.lines.map((l) => [l.kind, l.oldNo, l.newNo])).toEqual([
      ["ctx", 1, 1],
      ["del", 2, undefined],
      ["add", undefined, 2],
      ["add", undefined, 3],
      ["ctx", 3, 4],
      ["meta", undefined, undefined],
    ]);
    expect(d.hunks[1].lines[0]).toMatchObject({ kind: "del", oldNo: 10 });
    expect(d.hunks[1].lines[1]).toMatchObject({ kind: "add", newNo: 11 });
  });

  it("treats content that merely looks like headers as content once inside a hunk", () => {
    const d = parseUnifiedDiff("@@ -1 +1 @@\n--- not a header\n+++ also content\n");
    expect(d.hunks[0].lines.map((l) => l.kind)).toEqual(["del", "add"]);
  });

  it("detects binary files and tolerates empty input", () => {
    expect(parseUnifiedDiff("diff --git a/x b/x\nBinary files a/x and b/x differ\n").binary).toBe(true);
    expect(parseUnifiedDiff("")).toEqual({ hunks: [], binary: false, additions: 0, deletions: 0 });
  });
});
