import type { GlobalEvent, SessionListItem, StopReason } from "../api/types";
import { needsContinue } from "../api/types";
import { showNotification } from "../platform";

export interface Notice {
  title: string;
  body: string;
}

export interface NotifyContext {
  /** Session the user is looking at, if any. */
  activeId: string | null;
  /** True when the window is hidden or unfocused. */
  windowHidden: boolean;
  sessions: SessionListItem[];
}

const sessionName = (ctx: NotifyContext, id: string) =>
  ctx.sessions.find((s) => s.id === id)?.title || "A session";

const STOP_TEXT: Record<StopReason, string> = {
  completed: "finished",
  max_tokens: "was cut off by the output limit",
  max_steps: "stopped at the step limit",
  cancelled: "was cancelled",
  error: "failed",
};

/**
 * Decide whether a global event deserves an OS notification. Things the user is already looking at
 * stay quiet; everything else that needs them (approvals) or tells them an agent is done does not.
 */
export function noticeFor(event: GlobalEvent, ctx: NotifyContext): Notice | null {
  const watching = (id: string) => ctx.activeId === id && !ctx.windowHidden;
  switch (event.type) {
    case "permission_requested":
      if (watching(event.session_id)) return null;
      return {
        title: `${sessionName(ctx, event.session_id)} needs approval`,
        body: `${event.request.tool_name}: ${event.request.description}`,
      };
    case "turn_finished":
      // Cancelling is the user's own action; they do not need to be told.
      if (event.reason === "cancelled" || watching(event.session_id)) return null;
      return {
        title: `${sessionName(ctx, event.session_id)} ${STOP_TEXT[event.reason]}`,
        body: event.message ?? (needsContinue(event.reason) ? "Open it and send “continue”." : ""),
      };
    default:
      return null;
  }
}

/** Window title that surfaces how many sessions are waiting for approval. */
export function titleFor(sessions: SessionListItem[]): string {
  const waiting = sessions.reduce((n, s) => n + (s.pending_permission_count > 0 ? 1 : 0), 0);
  return waiting > 0 ? `(${waiting}) MauKode` : "MauKode";
}

const PREF_KEY = "maukode.notifications";

export function notificationsSupported(): boolean {
  return typeof Notification !== "undefined";
}

export function loadNotificationsPref(): boolean {
  try {
    return localStorage.getItem(PREF_KEY) === "1" && notificationsSupported() && Notification.permission === "granted";
  } catch {
    return false;
  }
}

/** Ask the browser/webview for permission and remember the choice. */
export async function enableNotifications(): Promise<boolean> {
  if (!notificationsSupported()) return false;
  const permission = Notification.permission === "granted" ? "granted" : await Notification.requestPermission();
  const on = permission === "granted";
  try {
    localStorage.setItem(PREF_KEY, on ? "1" : "0");
  } catch {
    /* preference just won't persist */
  }
  return on;
}

export function disableNotifications(): void {
  try {
    localStorage.setItem(PREF_KEY, "0");
  } catch {
    /* ignore */
  }
}

export function showNotice(n: Notice, onClick?: () => void): void {
  if (!notificationsSupported() || Notification.permission !== "granted") return;
  showNotification({
    title: n.title,
    body: n.body,
    onClick,
  });
}
