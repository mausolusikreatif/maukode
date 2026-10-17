import { useState } from "react";
import { Plug, Terminal } from "lucide-react";
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
    <div className="relative flex h-screen w-screen items-center justify-center overflow-hidden bg-background p-4 safe-top safe-bottom">
      {/* Ambient glow — the one bold element: a soft mint halo behind the card */}
      <div
        aria-hidden="true"
        className="pointer-events-none absolute -top-20 left-1/2 size-[32rem] -translate-x-1/2 rounded-full bg-primary/10 blur-[120px]"
      />
      <div className="relative w-full max-w-sm space-y-6 rounded-xl border border-border/60 bg-card/80 p-8 shadow-xl backdrop-blur-sm safe-left safe-right">
        <div className="flex flex-col items-center gap-3 text-center">
          <div className="flex size-14 items-center justify-center rounded-xl bg-primary/10 ring-1 ring-primary/20">
            <Terminal className="size-7 text-primary" />
          </div>
          <div className="space-y-1">
            <h1 className="text-xl font-semibold tracking-tight">MauKode</h1>
            <p className="font-mono text-xs text-muted-foreground">agentic coding assistant</p>
          </div>
        </div>

        <form
          className="space-y-4"
          onSubmit={(e) => {
            e.preventDefault();
            void connect(url, token || undefined);
          }}
        >
          <div className="space-y-2">
            <Label htmlFor="connect-url">Server URL</Label>
            <Input
              id="connect-url"
              data-testid="connect-url"
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="http://127.0.0.1:4096"
              autoCapitalize="none"
              autoCorrect="off"
              spellCheck={false}
            />
          </div>

          <div className="space-y-2">
            <Label htmlFor="connect-token">
              Token <span className="text-muted-foreground">(if the server requires one)</span>
            </Label>
            <Input
              id="connect-token"
              type="password"
              data-testid="connect-token"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="Paste token if required"
              autoComplete="off"
            />
          </div>

          {connectionError && (
            <div className="notice error rounded-md border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive" role="alert">
              {connectionError}
            </div>
          )}

          <Button data-testid="connect-button" type="submit" disabled={busy} className="w-full">
            {busy ? (
              <>
                <Plug className="size-4 animate-pulse" /> Connecting…
              </>
            ) : (
              <>
                <Plug className="size-4" /> Connect
              </>
            )}
          </Button>

          <p className="text-xs text-muted-foreground">
            Tip: bookmark{" "}
            <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs">
              {url.replace(/\/+$/, "")}#token=…
            </code>{" "}
            to auto-connect on future visits.
          </p>
        </form>
      </div>
    </div>
  );
}
