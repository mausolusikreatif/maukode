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

export async function pickDirectory(defaultPath?: string): Promise<string | null> {
  if (!window.__TAURI_INTERNALS__) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const selected = await open({
    directory: true,
    multiple: false,
    defaultPath: defaultPath || undefined,
    title: "Choose a project repository",
  });
  return typeof selected === "string" ? selected : null;
}

export function isDesktop(): boolean {
  return Boolean(window.__TAURI_INTERNALS__);
}

export function forgetBootstrap(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
}

/** Focus the application window, unminimizing and bringing it to the front if running as desktop app. */
export async function focusWindow(): Promise<void> {
  if (isDesktop()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("focus_window");
    } catch {
      /* ignore */
    }
  }
  window.focus();
}

export interface NativeNotificationOptions {
  title: string;
  body: string;
  sessionId?: string;
  onClick?: () => void;
}

/**
 * Dispatch a notification across native desktop and web.
 * Dispatches to OS notifications where supported and sets up click handling to focus window.
 */
export function showNotification(options: NativeNotificationOptions): void {
  // Native desktop notification dispatch if running inside Tauri
  if (isDesktop()) {
    void (async () => {
      try {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("send_notification", {
          title: options.title,
          body: options.body,
        });
      } catch {
        /* ignore */
      }
    })();
  }

  // Web Notification API (supported in browsers and WebViews with permission)
  if (typeof Notification !== "undefined" && Notification.permission === "granted") {
    try {
      const note = new Notification(options.title, { body: options.body });
      if (options.onClick) {
        note.onclick = () => {
          void focusWindow();
          options.onClick?.();
        };
      }
    } catch {
      /* ignore */
    }
  }
}

/**
 * Open a file or repository directory in an external code editor (VS Code or Cursor).
 */
export async function openInEditor(
  path: string,
  editor: "code" | "cursor" = "code"
): Promise<{ success: boolean; error?: string }> {
  if (isDesktop()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("open_in_editor", { path, editor });
      return { success: true };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return { success: false, error: msg };
    }
  }

  // Browser fallback: attempt custom URL protocol scheme
  try {
    const protocol = editor === "cursor" ? "cursor" : "vscode";
    window.open(`${protocol}://file/${encodeURI(path)}`);
    return { success: true };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return { success: false, error: msg };
  }
}

/**
 * Reveal a file or directory in the operating system's native file manager.
 */
export async function revealInFileManager(
  path: string
): Promise<{ success: boolean; error?: string }> {
  if (isDesktop()) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("reveal_in_file_manager", { path });
      return { success: true };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      return { success: false, error: msg };
    }
  }

  return {
    success: false,
    error: "Revealing in file manager is only available in the desktop application.",
  };
}

