import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { expect, type Page } from "@playwright/test";

export const env = () => ({
  daemon: process.env.E2E_DAEMON_URL!,
  token: process.env.E2E_TOKEN!,
  gateway: process.env.E2E_GATEWAY_URL!,
});

const json = (o: unknown) => JSON.stringify(o);
export const text = (t: string) => json({ choices: [{ delta: { content: t } }] });
export const finish = (reason: string) => json({ choices: [{ finish_reason: reason }] });
export const toolCall = (id: string, name: string, args: object, index = 0) =>
  json({
    choices: [{ delta: { tool_calls: [{ index, id, type: "function", function: { name, arguments: JSON.stringify(args) } }] } }],
  });

export type Step = { sse: string[] } | { status: number };

export async function script(steps: Step[]) {
  const res = await fetch(`${env().gateway}/__script`, { method: "POST", body: json({ steps }) });
  expect(res.ok).toBe(true);
}

export async function gatewayRequests(): Promise<any[]> {
  return (await fetch(`${env().gateway}/__requests`)).json();
}

/** A throwaway git repo with one commit, so worktree isolation works. */
export function gitRepo(): string {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "maukode-repo-"));
  const git = (...args: string[]) => execFileSync("git", args, { cwd: dir, stdio: "ignore" });
  git("init", "-q", "-b", "main");
  git("config", "user.email", "t@t");
  git("config", "user.name", "t");
  fs.writeFileSync(path.join(dir, "README.md"), "hi\n");
  git("add", "-A");
  git("commit", "-q", "-m", "init");
  return dir;
}

export async function api(method: string, p: string, body?: unknown) {
  const { daemon, token } = env();
  const res = await fetch(`${daemon}${p}`, {
    method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : json(body),
  });
  if (!res.ok) throw new Error(`${method} ${p}: ${res.status} ${await res.text()}`);
  const t = await res.text();
  return t ? JSON.parse(t) : undefined;
}

/** Connect through the UI and open a fresh session in `dir`. */
export async function connectAndCreate(page: Page, dir: string, opts: { isolate?: boolean } = {}) {
  const { daemon, token } = env();
  await page.goto("/");
  await page.getByTestId("connect-url").fill(daemon);
  await page.getByTestId("connect-token").fill(token);
  await page.getByTestId("connect-button").click();
  await expect(page.getByTestId("new-session")).toBeVisible();
  await createSession(page, dir, opts);
}

/** Open the "New session" dialog and create a session in `dir` (already connected). */
export async function createSession(page: Page, dir: string, opts: { isolate?: boolean } = {}) {
  await page.getByTestId("new-session").click();
  await page.getByTestId("dir-input").fill(dir);
  const iso = page.getByTestId("isolation-checkbox");
  if ((await iso.isChecked()) !== (opts.isolate ?? false)) await iso.click();
  await page.getByTestId("provider-select").selectOption("mock");
  await page.getByTestId("model-input").fill("m");
  await page.getByTestId("create-session").click();
  await expect(page.getByTestId("composer-input")).toBeVisible();
}

export async function send(page: Page, prompt: string) {
  await page.getByTestId("composer-input").fill(prompt);
  await page.getByTestId("send-button").click();
}

/** Send a prompt and wait for the turn to finish. */
export async function runTurn(page: Page, prompt: string) {
  await send(page, prompt);
  await expect(page.getByTestId("run-state")).toHaveText("idle");
}
