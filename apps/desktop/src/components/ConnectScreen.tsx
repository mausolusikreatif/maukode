import { useState } from "react";
import { useApp } from "../state/store";

export function ConnectScreen({ initialUrl, initialToken }: { initialUrl?: string; initialToken?: string }) {
  const [url, setUrl] = useState(initialUrl ?? "http://127.0.0.1:4096");
  const [token, setToken] = useState(initialToken ?? "");
  const { connect, connection, connectionError } = useApp();
  const busy = connection === "connecting";

  return (
    <form
      className="connect"
      onSubmit={(e) => {
        e.preventDefault();
        void connect(url, token || undefined);
      }}
    >
      <h1>MauKode</h1>
      <p className="muted">Connect to a running MauKode daemon.</p>
      <label>
        Server URL
        <input data-testid="connect-url" value={url} onChange={(e) => setUrl(e.target.value)} />
      </label>
      <label>
        Token <span className="muted">(if the server requires one)</span>
        <input
          data-testid="connect-token"
          type="password"
          value={token}
          onChange={(e) => setToken(e.target.value)}
          autoComplete="off"
        />
      </label>
      {connectionError && <div className="notice error">{connectionError}</div>}
      <button data-testid="connect-button" className="primary" disabled={busy}>
        {busy ? "Connecting…" : "Connect"}
      </button>
    </form>
  );
}
