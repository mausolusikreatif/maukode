/** Connection details for the MauKode daemon. */
export interface Bootstrap {
  url: string;
  token?: string;
}

const STORAGE_KEY = "maukode.connection";

/**
 * Where to connect on startup, in priority order: URL parameters (`?url=...&token=...`, handy for development), a
 * bookmarkable token in the URL fragment (`#token=...`, never sent to the server), the last
 * connection saved in this browser, and finally the server that served this page (so a browser
 * hitting the web UI served by maukode-server connects automatically).
 */
export async function getBootstrap(): Promise<Bootstrap | null> {
  // Dev mode: explicit ?url=...&token=...
  const params = new URLSearchParams(window.location.search);
  const url = params.get("url");
  if (url) return { url, token: params.get("token") ?? undefined };
  // Token from URL fragment (#token=...): bookmarkable for remote/Tailscale access,
  // and the fragment is never sent to the server (no logs, no server-side history).
  const hashParams = new URLSearchParams(window.location.hash.slice(1));
  const hashToken = hashParams.get("token");
  if (hashToken) {
    // Strip the fragment from the address bar so the token doesn't linger visibly.
    history.replaceState(null, "", window.location.pathname + window.location.search);
    return { url: window.location.origin, token: hashToken };
  }
  // Last saved connection.
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved) return JSON.parse(saved) as Bootstrap;
  } catch {
    /* storage unavailable */
  }
  // When the UI is served by maukode-server itself, default to that server.
  return { url: window.location.origin, token: undefined };
}

export function saveBootstrap(b: Bootstrap): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(b));
  } catch {
    /* storage unavailable (private mode): the user just re-enters it next time */
  }
}

/** True when running on macOS/iOS, where the primary shortcut modifier is Command. */
export function isMacPlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  const platform = navigator.platform ?? "";
  const userAgent = navigator.userAgent ?? "";
  return /Mac|iPhone|iPad|iPod/.test(platform) || /Mac OS X/.test(userAgent);
}

/** The primary shortcut modifier label for the current platform. */
export function primaryMod(): "Cmd" | "Ctrl" {
  return isMacPlatform() ? "Cmd" : "Ctrl";
}

/** True when the element is a text-entry surface, where global single-key shortcuts must not fire. */
export function isEditableTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el || typeof el.tagName !== "string") return false;
  return (
    el.tagName === "INPUT" ||
    el.tagName === "TEXTAREA" ||
    el.tagName === "SELECT" ||
    el.isContentEditable === true
  );
}

export function forgetBootstrap(): void {
  try {
    localStorage.removeItem(STORAGE_KEY);
  } catch {
    /* ignore */
  }
  // Also strip a token fragment from the address bar so it doesn't linger.
  if (window.location.hash.includes("token=")) {
    history.replaceState(null, "", window.location.pathname + window.location.search);
  }
}

/** Bring this browser tab to the front where the browser allows it. */
export async function focusWindow(): Promise<void> {
  if (typeof window !== "undefined" && typeof window.focus === "function") {
    window.focus();
  }
}

export interface NativeNotificationOptions {
  title: string;
  body: string;
  sessionId?: string;
  onClick?: () => void;
}

/** Show a web notification when permission has been granted. */
export function showNotification(options: NativeNotificationOptions): void {
  if (typeof Notification === "undefined" || Notification.permission !== "granted") return;
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
