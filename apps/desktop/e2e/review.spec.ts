import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import { connectAndCreate, createSession, finish, gitRepo, runTurn, script, text, toolCall } from "./helpers";

const git = (dir: string, ...args: string[]) => execFileSync("git", args, { cwd: dir }).toString().trim();
const read = (dir: string, file: string) => fs.readFileSync(path.join(dir, file), "utf8");

/** A turn in which the agent overwrites `file` and then finishes. */
const writeThenFinish = (file: string, content: string) => [
  { sse: [toolCall("c1", "write_file", { path: file, content }), finish("tool_calls")] },
  { sse: [text("done"), finish("stop")] },
];

test("review the agent's changes file by file, then undo them", async ({ page }) => {
  const repo = gitRepo();
  await script([
    {
      sse: [
        toolCall("c1", "write_file", { path: "new.txt", content: "hello\nworld\n" }, 0),
        toolCall("c2", "write_file", { path: "README.md", content: "hi\nthere\n" }, 1),
        finish("tool_calls"),
      ],
    },
    { sse: [text("done"), finish("stop")] },
  ]);
  await connectAndCreate(page, repo);
  await runTurn(page, "make changes");

  await page.getByTestId("toggle-review").click();
  await expect(page.getByTestId("diff-file")).toHaveCount(2);
  await expect(page.getByTestId("review-summary")).toContainText("2 files");
  await expect(page.getByTestId("review-summary")).toContainText("+3");

  // New file: added, both lines green.
  await page.getByTestId("diff-file").filter({ hasText: "new.txt" }).click();
  await expect(page.locator('[data-testid="diff-view"] [data-kind="add"]')).toHaveCount(2);
  await expect(page.getByTestId("diff-view")).toContainText("hello");

  // Existing file: modified, one added line, existing line kept as context.
  await page.getByTestId("diff-file").filter({ hasText: "README.md" }).click();
  await expect(page.locator('[data-testid="diff-view"] [data-kind="add"]')).toHaveCount(1);
  await expect(page.locator('[data-testid="diff-view"] [data-kind="ctx"]')).toHaveCount(1);
  await expect(page.getByTestId("diff-view")).toContainText("there");

  // Undo restores files on disk, and the panel empties.
  page.once("dialog", (d) => void d.accept());
  await page.getByTestId("undo-button").click();
  await expect(page.getByTestId("review-result")).toContainText(/reverted|restored/i);
  await expect(page.getByTestId("no-changes")).toBeVisible();
  expect(fs.existsSync(path.join(repo, "new.txt"))).toBe(false);
  expect(read(repo, "README.md")).toBe("hi\n");
});

test("commit and merge an isolated session into the project", async ({ page }) => {
  const repo = gitRepo();
  await script(writeThenFinish("README.md", "hi\nfrom agent\n"));
  await connectAndCreate(page, repo, { isolate: true });
  await runTurn(page, "edit the readme");

  await page.getByTestId("toggle-review").click();
  await expect(page.getByTestId("diff-file")).toHaveCount(1);
  await expect(page.getByTestId("worktree")).toContainText("Uncommitted changes");
  expect(read(repo, "README.md")).toBe("hi\n"); // the project is untouched until merge

  await page.getByTestId("commit-message").fill("agent edit");
  await page.getByTestId("commit-button").click();
  await expect(page.getByTestId("review-result")).toContainText("Committed");
  await expect(page.getByTestId("worktree")).toContainText("Clean");
  await expect(page.getByTestId("worktree")).toContainText("1 commit ahead");

  await page.getByTestId("merge-button").click();
  await expect(page.getByTestId("review-result")).toContainText("Merged into the project");
  expect(read(repo, "README.md")).toBe("hi\nfrom agent\n");
  // `--no-ff`: a real merge commit (two parents) that carries the message from the panel.
  expect(git(repo, "rev-list", "--parents", "-n", "1", "HEAD").split(" ")).toHaveLength(3);
  expect(git(repo, "log", "-1", "--format=%s")).toBe("agent edit");
  expect(git(repo, "status", "--porcelain")).toBe("");
});

test("a merge conflict is reported and leaves the project untouched", async ({ page }) => {
  const repo = gitRepo();
  await script([...writeThenFinish("README.md", "first\n"), ...writeThenFinish("README.md", "second\n")]);
  await connectAndCreate(page, repo, { isolate: true });
  await runTurn(page, "agent one");
  await createSession(page, repo, { isolate: true });
  await runTurn(page, "agent two");

  // Merge session two first so session one conflicts.
  await page.getByTestId("toggle-review").click();
  await page.getByTestId("merge-button").click();
  await expect(page.getByTestId("review-result")).toContainText("Merged into the project");
  expect(read(repo, "README.md")).toBe("second\n");

  await page.getByTestId("session-item").nth(1).click();
  await expect(page.getByTestId("worktree")).toBeVisible();
  await page.getByTestId("merge-button").click();
  await expect(page.getByTestId("review-result")).toContainText("Merge conflict");
  await expect(page.getByTestId("conflict-files")).toContainText("README.md");
  expect(read(repo, "README.md")).toBe("second\n");
  expect(git(repo, "status", "--porcelain")).toBe(""); // aborted cleanly
});

test("merging is refused while the project has uncommitted changes", async ({ page }) => {
  const repo = gitRepo();
  await script(writeThenFinish("notes.txt", "agent notes\n"));
  await connectAndCreate(page, repo, { isolate: true });
  await runTurn(page, "write notes");

  fs.writeFileSync(path.join(repo, "README.md"), "my local edit\n"); // tracked file, uncommitted
  await page.getByTestId("toggle-review").click();
  await page.getByTestId("merge-button").click();
  await expect(page.getByTestId("review-result")).toContainText(/uncommitted/i);
  expect(read(repo, "README.md")).toBe("my local edit\n");
  expect(fs.existsSync(path.join(repo, "notes.txt"))).toBe(false);
});

test("deleting a worktree session can keep or discard its branch", async ({ page }) => {
  const repo = gitRepo();
  await script([...writeThenFinish("a.txt", "a\n"), ...writeThenFinish("b.txt", "b\n")]);
  await connectAndCreate(page, repo, { isolate: true });
  await runTurn(page, "first");
  await page.getByTestId("toggle-review").click();
  await page.getByTestId("commit-button").click();
  await expect(page.getByTestId("review-result")).toContainText("Committed");
  const branchOne = (await page.getByTestId("worktree").locator("code").first().textContent())!;

  // The daemon is shared with other tests, so count sessions relative to now.
  const items = page.getByTestId("session-item");
  let remaining = await items.count();

  // Default: remove the checkout, keep the branch (unmerged work stays recoverable).
  await page.getByTestId("delete-session").click();
  await page.getByTestId("delete-confirm").click();
  remaining -= 1;
  await expect(items).toHaveCount(remaining);
  expect(git(repo, "branch", "--list", branchOne)).toContain(branchOne);

  await createSession(page, repo, { isolate: true });
  await runTurn(page, "second");
  const branchTwo = (await page.getByText(/⎇ maukode\//).first().textContent())!.replace(/.*⎇\s*/, "").split(" ")[0];
  await page.getByTestId("delete-session").click();
  await page.getByTestId("delete-branch").check();
  await page.getByTestId("delete-confirm").click();
  await expect(items).toHaveCount(remaining);
  expect(git(repo, "branch", "--list", branchTwo)).toBe("");
});
