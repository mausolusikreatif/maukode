/** Connection details for the MauKode daemon. */
export interface Bootstrap {
  url: string;
  token?: string;
}

const STORAGE_KEY = "maukode.connection";

declare global {
  interface Window {
    __TAURI_INTERNALS__?: unknown;
  }
}

/**
 * Where to connect on startup, in priority order: the Tauri shell (which starts/finds the local
 * daemon and owns its token), URL parameters (`?url=...&token=...`, handy for development), then
 * the last connection saved in this browser.
 */
export async function getBootstrap(): Promise<Bootstrap | null> {
  if (window.__TAURI_INTERNALS__) {
    const { invoke } = await import("@tauri-apps/api/core");
    return await invoke<Bootstrap>("daemon_info");
  }
  const params = new URLSearchParams(window.location.search);
  const url = params.get("url");
  if (url) return { url, token: params.get("token") ?? undefined };
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return saved ? (JSON.parse(saved) as Bootstrap) : null;
  } catch {
    return null;
  }
}

export function saveBootstrap(b: Bootstrap): void {
  // The Tauri shell keeps the token itself; only remember browser-mode connections.
  if (window.__TAURI_INTERNALS__) return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(b));
  } catch {
    /* storage unavailable (private mode): the user just re-enters it next time */
  }
}

export function forgetBootstrap(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}
