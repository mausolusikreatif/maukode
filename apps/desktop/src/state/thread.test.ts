import { describe, expect, it } from "vitest";
import type { LLMEvent } from "../api/types";
import { applyEvent, emptyThread, itemsFromMessages, resolvePermission, startTurn, type ThreadState } from "./thread";

const run = (events: LLMEvent[], from: ThreadState = emptyThread()) =>
  events.reduce((s, e, i) => applyEvent(s, e, i + 1), from);

describe("thread reducer", () => {
  it("streams text into one assistant item", () => {
    const s = run([
      { type: "text_delta", text: "Hel" },
      { type: "text_delta", text: "lo" },
    ]);
    expect(s.items).toEqual([{ kind: "assistant", id: "i1", text: "Hello" }]);
    expect(s.running).toBe(true);
    expect(s.lastSeq).toBe(2);
  });

  it("assembles a tool call and attaches its result", () => {
    const s = run([
      { type: "tool_call_start", id: "c1", name: "write_file" },
      { type: "tool_call_delta", id: "c1", delta: '{"path":' },
      { type: "tool_call_finish", tool_call: { id: "c1", name: "write_file", arguments: { path: "a" } } },
      { type: "tool_result", tool_call_id: "c1", tool_name: "write_file", content: "ok", is_error: false },
    ]);
    expect(s.items).toEqual([
      { kind: "tool", id: "c1", name: "write_file", args: '{"path":"a"}', status: "done", result: "ok" },
    ]);
  });

  it("marks failed and private results", () => {
    const base = run([{ type: "tool_call_start", id: "c", name: "bash" }]);
    const err = applyEvent(base, { type: "tool_result", tool_call_id: "c", tool_name: "bash", content: "x", is_error: true });
    expect((err.items[0] as { status: string }).status).toBe("error");
    const priv = applyEvent(base, { type: "private_tool_result", tool_call_id: "c", tool_name: "bash", content: "x", is_error: false });
    expect((priv.items[0] as { status: string }).status).toBe("private");
  });

  it("only turn_stopped ends the turn; step-level finish does not", () => {
    let s = run([{ type: "text_delta", text: "a" }, { type: "finish", reason: "end_turn" }]);
    expect(s.running).toBe(true);
    s = applyEvent(s, { type: "turn_stopped", reason: "completed", message: null }, 9);
    expect(s.running).toBe(false);
    expect(s.stop).toEqual({ reason: "completed", message: null });
    expect(s.notices).toEqual([]);
  });

  it("surfaces max_tokens / max_steps with a warning and keeps the reason for 'Continue'", () => {
    const s = run([{ type: "turn_stopped", reason: "max_steps", message: "Reached the step limit" }]);
    expect(s.stop?.reason).toBe("max_steps");
    expect(s.notices).toEqual([{ id: "n1", level: "warn", text: "Reached the step limit" }]);
  });

  it("dedupes permission prompts, clears them when the turn stops, and resolves locally", () => {
    const req = {
      type: "permission_request" as const, request_id: "r1", session_id: "s", tool_name: "bash",
      command: "ls", path: null, risk_level: "warning", description: "d",
    };
    let s = run([req, req]);
    expect(s.pending).toHaveLength(1);
    expect(resolvePermission(s, "r1").pending).toHaveLength(0);
    s = applyEvent(s, { type: "turn_stopped", reason: "cancelled", message: null });
    expect(s.pending).toHaveLength(0);
  });

  it("startTurn adds the user message and resets the stop reason", () => {
    const stopped = run([{ type: "turn_stopped", reason: "max_tokens", message: "m" }]);
    const s = startTurn(stopped, "continue");
    expect(s.running).toBe(true);
    expect(s.stop).toBeNull();
    expect(s.notices).toEqual([]);
    expect(s.items[s.items.length - 1]).toMatchObject({ kind: "user", text: "continue" });
  });

  it("builds items from stored history including tool results", () => {
    const items = itemsFromMessages([
      { role: "user", parts: [{ type: "text", text: "go" }] },
      { role: "assistant", parts: [{ type: "text", text: "ok" }, { type: "tool_call", id: "c", name: "bash", arguments: { command: "ls" } }] },
      { role: "user", parts: [{ type: "tool_result", tool_call_id: "c", tool_name: "bash", content: "files", is_error: false }] },
    ]);
    expect(items.map((i) => i.kind)).toEqual(["user", "assistant", "tool"]);
    expect(items[2]).toMatchObject({ status: "done", result: "files", args: '{"command":"ls"}' });
  });
});
