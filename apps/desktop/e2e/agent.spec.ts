import fs from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import { api, connectAndCreate, finish, gatewayRequests, gitRepo, radSelect, script, send, text, toolCall } from "./helpers";

test("approve a tool call, see the result and the finished answer", async ({ page }) => {
  const repo = gitRepo();
  await script([
    // finish_reason "stop" on a step that carries a tool call: a real-world gateway quirk.
    { sse: [toolCall("c1", "write_file", { path: "out.txt", content: "hello" }), finish("stop")] },
    { sse: [text("All done."), finish("stop")] },
  ]);
  await connectAndCreate(page, repo, { isolate: true });

  // Worktree isolation is visible in the header and the sidebar.
  await expect(page.getByText(/⎇ maukode\//).first()).toBeVisible();

  await radSelect(page, "perm-mode", "strict");
  await send(page, "write a file");

  await expect(page.getByTestId("permission-card")).toBeVisible();
  await expect(page.getByTestId("needs-approval")).toHaveText(/1 approval/);
  await expect(page.getByTestId("summary")).toContainText("1 need you");
  await expect(page.getByTestId("run-state")).toHaveText("running");

  await page.getByTestId("perm-allow-once").click();

  await expect(page.getByTestId("assistant-msg")).toHaveText("All done.");
  await expect(page.getByTestId("tool-status")).toHaveText("done");
  await expect(page.getByTestId("run-state")).toHaveText("idle");
  await expect(page.getByTestId("permission-card")).toHaveCount(0);
  await expect(page.getByTestId("stop-banner")).toHaveCount(0);

  // The agent wrote into its isolated worktree, not into the project checkout.
  expect(fs.existsSync(path.join(repo, "out.txt"))).toBe(false);
  const reqs = await gatewayRequests();
  expect(reqs).toHaveLength(2);
});

test("a cut-off task shows a Continue button that resumes it", async ({ page }) => {
  const repo = gitRepo();
  // Initial response plus both automatic continuations are cut off; the third is the user's.
  await script([
    { sse: [text("a"), finish("length")] },
    { sse: [text("b"), finish("length")] },
    { sse: [text("c"), finish("length")] },
    { sse: [text(" and the rest."), finish("stop")] },
  ]);
  await connectAndCreate(page, repo);
  await send(page, "write a long essay");

  await expect(page.getByTestId("stop-banner")).toBeVisible();
  await expect(page.getByTestId("run-state")).toHaveText("idle");
  await expect(page.getByTestId("notice").first()).toContainText("output limit");

  await page.getByTestId("continue-button").click();
  await expect(page.getByTestId("stop-banner")).toHaveCount(0);
  await expect(page.getByTestId("assistant-msg").last()).toContainText("and the rest.");
  const last = (await gatewayRequests()).at(-1);
  expect(last.messages.at(-1)).toMatchObject({ role: "user", content: "continue" });
});

test("a provider failure is shown and the session stays usable", async ({ page }) => {
  const repo = gitRepo();
  // Transient failures are retried inside one step (up to 4 attempts), so the
  // script must fail persistently to exercise the visible-error path.
  await script([
    { status: 500 },
    { status: 500 },
    { status: 500 },
    { status: 500 },
    { sse: [text("back again"), finish("stop")] },
  ]);
  await connectAndCreate(page, repo);

  await send(page, "first");
  await expect(page.getByTestId("notice").first()).toContainText(/500|error/i);
  await expect(page.getByTestId("run-state")).toHaveText("idle");

  await send(page, "second");
  await expect(page.getByTestId("assistant-msg")).toHaveText("back again");
  await expect(page.getByTestId("notice")).toHaveCount(0);
});

test("Stop works while an approval prompt is open", async ({ page }) => {
  const repo = gitRepo();
  await script([
    { sse: [toolCall("c1", "write_file", { path: "x.txt", content: "x" }), finish("tool_calls")] },
    { sse: [text("ok"), finish("stop")] },
  ]);
  await connectAndCreate(page, repo);
  await radSelect(page, "perm-mode", "strict");
  await send(page, "go");
  await expect(page.getByTestId("permission-card")).toBeVisible();

  await page.getByTestId("stop-button").click();
  await expect(page.getByTestId("run-state")).toHaveText("idle");
  await expect(page.getByTestId("permission-card")).toHaveCount(0);
  expect(fs.existsSync(path.join(repo, "x.txt"))).toBe(false);

  // History is valid after a cancel, so the next message is accepted by the provider.
  await send(page, "continue please");
  await expect(page.getByTestId("assistant-msg").last()).toHaveText("ok");
});

test("reloading mid-turn restores the pending approval", async ({ page }) => {
  const repo = gitRepo();
  await script([
    { sse: [toolCall("c1", "write_file", { path: "y.txt", content: "y" }), finish("tool_calls")] },
    { sse: [text("saved"), finish("stop")] },
  ]);
  await connectAndCreate(page, repo);
  await radSelect(page, "perm-mode", "strict");
  await send(page, "go");
  await expect(page.getByTestId("permission-card")).toBeVisible();

  await page.reload(); // connection is remembered in this browser
  await expect(page.getByTestId("session-item").first()).toBeVisible();
  await page.getByTestId("session-item").first().click();
  await expect(page.getByTestId("permission-card")).toBeVisible();
  await expect(page.getByTestId("run-state")).toHaveText("running");

  await page.getByTestId("perm-allow-once").click();
  await expect(page.getByTestId("assistant-msg").last()).toHaveText("saved");
  expect(fs.existsSync(path.join(repo, "y.txt"))).toBe(true);
  await api("GET", "/api/v1/health"); // daemon still healthy
});
