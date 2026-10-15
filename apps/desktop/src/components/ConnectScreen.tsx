import { useState } from "react";
import { Plug } from "lucide-react";
import { useApp } from "../state/store";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { Label } from "./ui/label";

export function ConnectScreen({ initialUrl, initialToken }: { initialUrl?: string; initialToken?: string }) {
  const [url, setUrl] = useState(initialUrl ?? "http://127.0.0.1:4096");
  const [token, setToken] = useState(initialToken ?? "");
  const { connect, connection, connectionError } = useApp();
  const busy = connection === "connecting";

  return (
    <div className="flex h-screen w-screen items-center justify-center bg-background p-6">
      <form
        className="connect flex w-full max-w-sm flex-col gap-4 rounded-xl border border-border bg-card p-6 shadow-lg"
        onSubmit={(e) => {
          e.preventDefault();
          void connect(url, token || undefined);
        }}
      >
        <div className="flex items-center gap-2">
          <span className="flex size-8 items-center justify-center rounded-lg bg-primary/15 text-primary">
            <Plug className="size-4" />
          </span>
          <h1 className="text-lg font-semibold tracking-tight">MauKode</h1>
        </div>
        <p className="text-sm text-muted-foreground">Connect to a running MauKode daemon.</p>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="connect-url">Server URL</Label>
          <Input
            id="connect-url"
            data-testid="connect-url"
            value={url}
            onChange={(e) => setUrl(e.target.value)}
          />
        </div>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="connect-token">
            Token <span className="text-muted-foreground">(if the server requires one)</span>
          </Label>
          <Input
            id="connect-token"
            data-testid="connect-token"
            type="password"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            autoComplete="off"
          />
        </div>

        {connectionError && (
          <div className="notice error rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {connectionError}
          </div>
        )}

        <Button data-testid="connect-button" type="submit" disabled={busy}>
          {busy ? "Connecting…" : "Connect"}
        </Button>

        <p className="text-xs text-muted-foreground">
          Tip: bookmark{" "}
          <code className="rounded bg-muted px-1 py-0.5 text-[0.7rem]">
            {url.replace(/\/+$/, "")}#token=…
          </code>{" "}
          to auto-connect on future visits.
        </p>
      </form>
    </div>
  );
}
