import { describe, expect, it } from "vitest";
import type { GlobalEvent, SessionListItem, StopReason } from "../api/types";
import { noticeFor, titleFor, type NotifyContext } from "./notify";

const session = (id: string, over: Partial<SessionListItem> = {}): SessionListItem => ({
  id, directory: "/p", title: `Task ${id}`, message_count: 1, provider: "p", model: "m",
  updated_at: "2026-01-01T00:00:00Z", project_dir: null, worktree_branch: null,
  running: false, pending_permission_count: 0,
  turn_started_at: null, plan_done: 0, plan_total: 0, last_activity: null, ...over,
});

const ctx = (over: Partial<NotifyContext> = {}): NotifyContext => ({
  activeId: "a", windowHidden: false, sessions: [session("a"), session("b")], ...over,
});

const approval = (id: string): GlobalEvent => ({
  type: "permission_requested", session_id: id,
  request: { request_id: "r", session_id: id, tool_name: "bash", command: "ls", path: null, risk_level: "warning", description: "run ls" },
});
const finished = (id: string, reason: StopReason, message: string | null = null): GlobalEvent => ({
  type: "turn_finished", session_id: id, reason, message,
});

describe("noticeFor", () => {
  it("stays quiet for the session the user is watching, but not when the window is hidden", () => {
    expect(noticeFor(approval("a"), ctx())).toBeNull();
    expect(noticeFor(approval("a"), ctx({ windowHidden: true }))?.title).toBe("Task a needs approval");
  });

  it("notifies for approvals in other sessions", () => {
    const n = noticeFor(approval("b"), ctx());
    expect(n).toEqual({ title: "Task b needs approval", body: "bash: run ls" });
  });

  it("explains how a background turn ended, and never nags about user cancels", () => {
    expect(noticeFor(finished("b", "completed"), ctx())?.title).toBe("Task b finished");
    expect(noticeFor(finished("b", "error", "boom"), ctx())).toEqual({ title: "Task b failed", body: "boom" });
    expect(noticeFor(finished("b", "max_steps", "limit"), ctx())?.body).toBe("limit");
    expect(noticeFor(finished("b", "cancelled"), ctx())).toBeNull();
  });

  it("ignores events that need no attention", () => {
    expect(noticeFor({ type: "turn_started", session_id: "b" }, ctx())).toBeNull();
    expect(noticeFor({ type: "session_created", session_id: "b" }, ctx())).toBeNull();
  });
});

describe("titleFor", () => {
  it("counts sessions waiting for approval, not individual prompts", () => {
    expect(titleFor([session("a"), session("b")])).toBe("MauKode");
    expect(titleFor([session("a", { pending_permission_count: 3 }), session("b", { pending_permission_count: 1 })])).toBe("(2) MauKode");
  });
});
