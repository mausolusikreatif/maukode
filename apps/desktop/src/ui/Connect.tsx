import { useState } from "react";
import { useApp } from "../state/store";

/** Shown while connecting, and when the server is unreachable or rejects the token. */
export function Connect({ initialUrl }: { initialUrl: string }) {
  const { connection, connectionError, connect } = useApp();
  const [url, setUrl] = useState(initialUrl);
  const [token, setToken] = useState("");
  const busy = connection === "connecting";

  return (
    <main className="center">
      <form
        className="card"
        onSubmit={(e) => {
          e.preventDefault();
          void connect(url.trim(), token.trim() || undefined);
        }}
      >
        <h1 className="strong" style={{ fontSize: 20, margin: 0 }}>MauKode</h1>
        {busy ? (
          <p className="muted">Menghubungkan ke {url}…</p>
        ) : (
          <p className="muted">
            {connectionError ?? "Masukkan alamat server dan token untuk mulai."}
          </p>
        )}
        {connectionError && <div className="notice error">{connectionError}</div>}
        <label className="label" htmlFor="url">Alamat server</label>
        <input id="url" className="field" value={url} onChange={(e) => setUrl(e.target.value)} inputMode="url" autoCapitalize="off" />
        <label className="label" htmlFor="token">Token</label>
        <input id="token" className="field" type="password" value={token} onChange={(e) => setToken(e.target.value)} autoCapitalize="off" autoComplete="off" />
        <button className="btn send" disabled={busy} type="submit">Hubungkan</button>
      </form>
    </main>
  );
}
