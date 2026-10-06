import { spawn, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** Scripted OpenAI-compatible gateway so UI tests never need a live LLM account. */
type Step = { sse: string[] } | { status: number };

function startGateway(): Promise<{ server: http.Server; url: string }> {
  let script: Step[] = [];
  let requests: unknown[] = [];
  const server = http.createServer((req, res) => {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      const url = req.url ?? "";
      if (req.method === "POST" && url === "/__script") {
        script = JSON.parse(body).steps;
        requests = [];
        res.end("ok");
      } else if (req.method === "GET" && url === "/__requests") {
        res.setHeader("content-type", "application/json");
        res.end(JSON.stringify(requests));
      } else if (req.method === "POST" && url === "/v1/chat/completions") {
        requests.push(JSON.parse(body));
        const step = script.shift();
        if (!step) {
          res.statusCode = 500;
          res.end("gateway script exhausted");
        } else if ("status" in step) {
          res.statusCode = step.status;
          res.end("scripted failure");
        } else {
          res.setHeader("content-type", "text/event-stream");
          res.end(step.sse.map((c) => `data: ${c}\n\n`).join("") + "data: [DONE]\n\n");
        }
      } else {
        res.statusCode = 404;
        res.end();
      }
    });
  });
  return new Promise((resolve) =>
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address() as net.AddressInfo;
      resolve({ server, url: `http://127.0.0.1:${port}` });
    }),
  );
}

function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const s = net.createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address() as net.AddressInfo;
      s.close(() => resolve(port));
    });
  });
}

async function waitFor(url: string, timeoutMs = 30_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {
      /* not up yet */
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  throw new Error(`timed out waiting for ${url}`);
}

export default async function globalSetup() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "maukode-e2e-"));
  const bin = process.env.MAUKODE_BIN ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../../target/debug/maukode");
  if (!fs.existsSync(bin)) {
    throw new Error(`MauKode binary not found at ${bin}; run \`cargo build -p maukode-cli\` or set MAUKODE_BIN`);
  }

  const gateway = await startGateway();
  const port = await freePort();
  const token = "e2e-token";
  const log = fs.openSync(path.join(root, "daemon.log"), "a");
  const daemon: ChildProcess = spawn(bin, ["server", "--bind", `127.0.0.1:${port}`, "--dir", root], {
    env: { ...process.env, MAUKODE_HOME: path.join(root, "home"), MAUKODE_AUTH_TOKEN: token },
    stdio: ["ignore", log, log],
  });
  const url = `http://127.0.0.1:${port}`;
  await waitFor(`${url}/api/v1/health`);

  // Register the scripted gateway as a provider.
  const res = await fetch(`${url}/api/v1/providers`, {
    method: "POST",
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: JSON.stringify({
      id: "mock", name: "Mock", kind: "open_ai_compatible",
      base_url: gateway.url, api_key: "k", default_model: "m",
    }),
  });
  if (!res.ok) throw new Error(`provider setup failed: ${res.status} ${await res.text()}`);

  Object.assign(process.env, {
    E2E_DAEMON_URL: url,
    E2E_TOKEN: token,
    E2E_GATEWAY_URL: gateway.url,
    E2E_ROOT: root,
  });

  return async () => {
    daemon.kill("SIGTERM");
    gateway.server.close();
    if (!process.env.E2E_KEEP) fs.rmSync(root, { recursive: true, force: true });
  };
}
