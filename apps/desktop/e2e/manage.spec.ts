import { expect, test, type Page } from "@playwright/test";
import { api, connectAndCreate, env, expectRadValue, finish, gitRepo, radSelect, script, send, text, toolCall } from "./helpers";

const approvalTurn = [
  { sse: [toolCall("c1", "write_file", { path: "n.txt", content: "x" }), finish("tool_calls")] },
  { sse: [text("done"), finish("stop")] },
];

/** Replace `Notification` with a recorder that is already permitted. */
async function stubNotifications(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { __notes: { title: string; body?: string }[]; Notification: unknown };
    w.__notes = [];
    class FakeNotification {
      static permission = "granted";
      static requestPermission = async () => "granted";
      onclick: (() => void) | null = null;
      constructor(title: string, opts?: { body?: string }) {
        w.__notes.push({ title, body: opts?.body });
      }
    }
    w.Notification = FakeNotification;
  });
}

const notes = (page: Page) => page.evaluate(() => (window as unknown as { __notes: { title: string; body?: string }[] }).__notes);

async function openSettings(page: Page) {
  await page.getByTestId("nav-settings").click();
  await expect(page.getByTestId("settings")).toBeVisible();
}

test("the overview surfaces what needs attention across agents", async ({ page }) => {
  const repo = gitRepo();
  await script(approvalTurn);
  await connectAndCreate(page, repo);
  await radSelect(page, "perm-mode", "strict");
  await send(page, "overview task alpha");
  await expect(page.getByTestId("permission-card")).toBeVisible();

  await page.getByTestId("nav-overview").click();
  const needs = page.getByTestId("dash-needs");
  await expect(needs).toContainText("overview task alpha");
  await expect(needs).toContainText("1 approval waiting");
  await expect(page).toHaveTitle(/^\(\d+\) MauKode$/);

  // Jump straight to the waiting prompt, approve, and the card moves out of "needs you".
  await needs.getByTestId("dash-card").first().click();
  await page.getByTestId("perm-allow-once").click();
  await expect(page.getByTestId("run-state")).toHaveText("idle");
  await page.getByTestId("nav-overview").click();
  await expect(page.getByTestId("dash-recent")).toContainText("overview task alpha");
  await expect(page).toHaveTitle("MauKode");
});

test("an agent that stopped early is flagged on the overview", async ({ page }) => {
  const repo = gitRepo();
  await script([
    { sse: [text("a"), finish("length")] },
    { sse: [text("b"), finish("length")] },
    { sse: [text("c"), finish("length")] },
  ]);
  await connectAndCreate(page, repo);
  await send(page, "overview task beta");
  await expect(page.getByTestId("stop-banner")).toBeVisible();

  await page.getByTestId("nav-overview").click();
  const stopped = page.getByTestId("dash-stopped");
  await expect(stopped).toContainText("overview task beta");
  await expect(stopped).toContainText("output limit");
});

test("desktop notifications fire for background agents, not for what you're watching", async ({ page }) => {
  await stubNotifications(page);
  const repo = gitRepo();
  await script(approvalTurn);
  await connectAndCreate(page, repo);
  await openSettings(page);
  await page.getByTestId("notifications-toggle").check();
  await expect(page.getByTestId("notifications-toggle")).toBeChecked();

  // A different agent, started behind the UI's back.
  const bg = await api("POST", "/api/v1/sessions", { directory: repo, provider: "mock", model: "m", title: "Background job" });
  await api("POST", "/api/v1/permissions/mode", { mode: "strict", session_id: bg.id });
  await expect(page.getByTestId("session-item").filter({ hasText: "Background job" })).toBeVisible();
  await api("POST", `/api/v1/sessions/${bg.id}/prompt`, { prompt: "do it" });

  await expect.poll(async () => (await notes(page)).map((n) => n.title)).toContain("Background job needs approval");
  await expect(page).toHaveTitle(/^\(\d+\) MauKode$/);

  const detail = await api("GET", `/api/v1/sessions/${bg.id}`);
  await api("POST", `/api/v1/sessions/${bg.id}/permissions/${detail.pending_permissions[0].request_id}`, { decision: "allow_once" });
  await expect.poll(async () => (await notes(page)).map((n) => n.title)).toContain("Background job finished");
});

test("settings: default approval mode and agent limits persist across reloads", async ({ page }) => {
  const repo = gitRepo();
  await connectAndCreate(page, repo);
  try {
    await openSettings(page);
    await radSelect(page, "default-mode", "strict");
    await expect(page.getByTestId("settings-msg")).toContainText("Default approval mode saved");

    await page.getByTestId("max-steps").fill("7");
    await page.getByTestId("max-tokens").fill("2048");
    await page.getByTestId("save-limits").click();
    await expect(page.getByTestId("settings-msg")).toContainText("Limits saved");

    // Out-of-range values are rejected by the server and explained.
    await page.getByTestId("max-steps").fill("0");
    await page.getByTestId("save-limits").click();
    await expect(page.getByTestId("settings-msg")).toContainText("max_steps must be");

    await page.reload();
    await openSettings(page);
    await expectRadValue(page, "default-mode", "strict");
    await expect(page.getByTestId("max-steps")).toHaveValue("7");
    await expect(page.getByTestId("max-tokens")).toHaveValue("2048");
  } finally {
    // Other tests share this daemon.
    await api("PUT", "/api/v1/config/security", { permission_mode: "ask" });
    await api("POST", "/api/v1/config/agent", { max_steps: 50, max_tokens: 8192 });
  }
});

test("settings: agent-limit fields wait for the server's values instead of being overwritten by them", async ({ page }) => {
  const repo = gitRepo();
  await connectAndCreate(page, repo);
  // Slow the config response down: a late reply used to clobber whatever had already been typed.
  await page.route("**/api/v1/config/agent", async (route) => {
    if (route.request().method() === "GET") await new Promise((r) => setTimeout(r, 1500));
    await route.continue();
  });
  await openSettings(page);
  await expect(page.getByTestId("max-steps")).toBeDisabled();
  await expect(page.getByTestId("save-limits")).toBeDisabled();
  await expect(page.getByTestId("max-steps")).toBeEnabled({ timeout: 10_000 });
  await expect(page.getByTestId("max-steps")).toHaveValue(/^\d+$/);
});

test("settings: add, edit and remove a provider without ever seeing its key", async ({ page }) => {
  const repo = gitRepo();
  await connectAndCreate(page, repo);
  await openSettings(page);

  await page.getByTestId("add-provider").click();
  await page.getByTestId("pf-id").fill("e2e-extra");
  await page.getByTestId("pf-name").fill("E2E Extra");
  await page.getByTestId("pf-base-url").fill("http://127.0.0.1:9");
  await page.getByTestId("pf-model").fill("model-one");
  await page.getByTestId("pf-key").fill("sk-secret-value");
  await page.getByTestId("pf-save").click();

  const row = page.getByTestId("provider-row").filter({ hasText: "E2E Extra" });
  await expect(row).toContainText("key stored");
  await expect(row).toContainText("model-one");
  expect(await page.content()).not.toContain("sk-secret-value");

  await row.getByTestId("edit-provider").click();
  await page.getByTestId("pf-model").fill("model-two");
  await page.getByTestId("pf-save").click();
  await expect(row).toContainText("model-two");
  await expect(row).toContainText("key stored"); // blank key field keeps the stored key

  page.once("dialog", (d) => void d.accept());
  await row.getByTestId("delete-provider").click();
  await expect(page.getByTestId("provider-row").filter({ hasText: "E2E Extra" })).toHaveCount(0);

  // The default provider can't be removed from the UI.
  const defaultRow = page.getByTestId("provider-row").filter({ has: page.locator(".badge.run") }).first();
  await expect(defaultRow.getByTestId("delete-provider")).toHaveCount(0);
});

test("settings: ChatGPT sign-in flow reports failure and can be cancelled", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as { __opened: string[] }).__opened = [];
    window.open = (u?: string | URL) => {
      (window as unknown as { __opened: string[] }).__opened.push(String(u));
      return null;
    };
  });
  const repo = gitRepo();
  await connectAndCreate(page, repo);
  await openSettings(page);
  await expect(page.getByTestId("chatgpt-signed-out")).toBeVisible();

  await page.getByTestId("chatgpt-signin").click();
  await expect(page.getByTestId("chatgpt-waiting")).toBeVisible();
  const opened = await page.evaluate(() => (window as unknown as { __opened: string[] }).__opened[0]);
  const url = new URL(opened);
  expect(url.origin + url.pathname).toBe("https://auth.openai.com/api/accounts/authorize");
  expect(url.searchParams.get("client_id")).toBe("dynamic_agent_client");
  expect(url.searchParams.get("code_challenge_method")).toBe("S256");

  // A forged callback (wrong state) must be refused and surfaced.
  await fetch(`${url.searchParams.get("redirect_uri")}?state=forged&code=x`);
  await expect(page.getByTestId("chatgpt-error")).toContainText("state mismatch");
  await expect(page.getByTestId("chatgpt-signin")).toHaveText("Try again");

  // Retry, then cancel: back to a clean signed-out state with no listener left behind.
  await page.getByTestId("chatgpt-signin").click();
  await expect(page.getByTestId("chatgpt-waiting")).toBeVisible();
  await page.getByTestId("chatgpt-cancel").click();
  await expect(page.getByTestId("chatgpt-waiting")).toHaveCount(0);
  await expect(page.getByTestId("chatgpt-signin")).toHaveText("Sign in with ChatGPT");
  const status = await api("GET", "/api/v1/auth/chatgpt");
  expect(status.accounts).toEqual([]);
  void env;
});

test("the audit trail lists approval decisions for a session", async ({ page }) => {
  const repo = gitRepo();
  await script(approvalTurn);
  await connectAndCreate(page, repo);
  await radSelect(page, "perm-mode", "strict");
  await send(page, "audited work");
  await page.getByTestId("perm-allow-once").click();
  await expect(page.getByTestId("run-state")).toHaveText("idle");

  await page.getByTestId("audit-button").click();
  const dialog = page.getByTestId("audit-dialog");
  await expect(dialog.getByTestId("audit-counts")).toContainText("1 approval decision");
  await expect(dialog.getByTestId("audit-permissions")).toContainText("write_file");
  await expect(dialog.getByTestId("audit-decision")).toHaveText("allow_once");
  await expect(dialog).toContainText("ask → strict");
});

test("command palette, keyboard shortcuts, and external editor actions", async ({ page }) => {
  const repo = gitRepo();
  await connectAndCreate(page, repo);

  // Repository action buttons are visible on the session header
  await expect(page.getByTestId("open-vscode")).toBeVisible();
  await expect(page.getByTestId("open-cursor")).toBeVisible();
  await expect(page.getByTestId("reveal-file-manager")).toBeVisible();

  // Open Command Palette with Ctrl+K
  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("command-palette")).toBeVisible();
  await expect(page.getByTestId("palette-input")).toBeFocused();

  // Search for Toggle Sidebar action
  await page.getByTestId("palette-input").fill("Toggle Sidebar");
  await expect(page.getByTestId("palette-item")).toHaveCount(1);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("command-palette")).not.toBeVisible();
  // Sidebar is now closed; toggle button appears in crumbs
  await expect(page.getByTestId("toggle-sidebar-btn")).toBeVisible();

  // Re-open palette and toggle sidebar back
  await page.keyboard.press("Control+k");
  await expect(page.getByTestId("command-palette")).toBeVisible();
  await page.getByTestId("palette-input").fill("Toggle Sidebar");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("new-session")).toBeVisible();

  // Open shortcut cheat sheet with '?'
  await page.keyboard.press("?");
  await expect(page.getByTestId("shortcut-modal")).toBeVisible();
  await page.getByTestId("close-shortcuts").click();
  await expect(page.getByTestId("shortcut-modal")).not.toBeVisible();
});

