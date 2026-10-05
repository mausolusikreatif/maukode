import fs from "node:fs";
import { defineConfig } from "@playwright/test";

// Prefer a preinstalled Chromium (sandbox image) over Playwright's own download; on CI the
// default managed browser from `playwright install` is used.
const preinstalled = "/opt/pw-browsers/chromium";
const executablePath = process.env.PW_CHROMIUM ?? (fs.existsSync(preinstalled) ? preinstalled : undefined);

export default defineConfig({
  testDir: "./e2e",
  testMatch: /.*\.spec\.ts/,
  timeout: 45_000,
  fullyParallel: false,
  workers: 1,
  reporter: [["list"]],
  globalSetup: "./e2e/global-setup.ts",
  webServer: {
    // Bind IPv4 explicitly: `localhost` may resolve to ::1 first (Node 17+), so the readiness check
    // against 127.0.0.1 would time out on hosts that list both addresses.
    command: "npx vite build && npx vite preview --host 127.0.0.1 --port 4173 --strictPort",
    url: "http://127.0.0.1:4173",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
  use: {
    baseURL: "http://127.0.0.1:4173",
    launchOptions: { executablePath, args: ["--no-sandbox"] },
    trace: "retain-on-failure",
  },
});
