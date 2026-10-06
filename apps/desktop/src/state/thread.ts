import { needsContinue } from "../api/types";
import type { ContentPart, LLMEvent, Message, PermissionRequest, Plan, StopReason, TokenUsage } from "../api/types";

export type ToolStatus = "running" | "done" | "error" | "private";

export type ThreadItem =
  | { kind: "user"; id: string; text: string }
  | { kind: "assistant"; id: string; text: string }
  | { kind: "reasoning"; id: string; text: string }
  | { kind: "plan"; id: string; plan: Plan }
  | { kind: "tool"; id: string; name: string; args: string; status: ToolStatus; result?: string };

/** Transient status lines for the current turn (errors, limits). Cleared when a new turn starts. */
export interface Notice {
  id: string;
  level: "warn" | "error";
  text: string;
}

export interface ThreadState {
  items: ThreadItem[];
  notices: Notice[];
  running: boolean;
  pending: PermissionRequest[];
  /** Why the last turn ended; cleared when a new turn starts. */
  stop: { reason: StopReason; message: string | null } | null;
  usage: TokenUsage | null;
  /** Active structured plan for the session, if one has been emitted. */
  plan: Plan | null;
  /** Queued prompts waiting for subsequent turns. */
  queue: string[];
  /** Newest event sequence applied; resume the SSE stream from here. */
  lastSeq: number;
  nextId: number;
}

export function emptyThread(): ThreadState {
  return { items: [], notices: [], running: false, pending: [], stop: null, usage: null, plan: null, queue: [], lastSeq: 0, nextId: 1 };
}

function pushItem(state: ThreadState, make: (id: string) => ThreadItem): ThreadState {
  const id = `i${state.nextId}`;
  return { ...state, items: [...state.items, make(id)], nextId: state.nextId + 1 };
}

function updateTool(state: ThreadState, id: string, patch: (t: Extract<ThreadItem, { kind: "tool" }>) => ThreadItem): ThreadState | null {
  const idx = state.items.findIndex((i) => i.kind === "tool" && i.id === id);
  if (idx === -1) return null;
  const items = state.items.slice();
  items[idx] = patch(items[idx] as Extract<ThreadItem, { kind: "tool" }>);
  return { ...state, items };
}

export function addNotice(state: ThreadState, level: Notice["level"], text: string): ThreadState {
  const id = `n${state.nextId}`;
  return { ...state, notices: [...state.notices, { id, level, text }], nextId: state.nextId + 1 };
}

function appendText(state: ThreadState, kind: "assistant" | "reasoning", text: string): ThreadState {
  const last = state.items[state.items.length - 1];
  if (last && last.kind === kind) {
    const items = state.items.slice();
    items[items.length - 1] = { ...last, text: last.text + text };
    return { ...state, items };
  }
  return pushItem(state, (id) => ({ kind, id, text }));
}

/** A user message the client just sent, shown before the server echoes anything back. */
export function startTurn(state: ThreadState, prompt: string): ThreadState {
  const next = pushItem(state, (id) => ({ kind: "user", id, text: prompt }));
  return { ...next, notices: [], running: true, stop: null };
}

/** Fold one server event into the thread. Pure: returns a new state. */
export function applyEvent(state: ThreadState, event: LLMEvent, seq?: number): ThreadState {
  let next: ThreadState = seq !== undefined && seq > state.lastSeq ? { ...state, lastSeq: seq } : state;
  switch (event.type) {
    case "text_delta":
      return { ...appendText(next, "assistant", event.text), running: true };
    case "reasoning_delta":
      return { ...appendText(next, "reasoning", event.text), running: true };
    case "tool_call_start": {
      if (next.items.some((i) => i.kind === "tool" && i.id === event.id)) return next;
      const item: ThreadItem = { kind: "tool", id: event.id, name: event.name, args: "", status: "running" };
      return { ...next, items: [...next.items, item], running: true };
    }
    case "tool_call_delta":
      return updateTool(next, event.id, (t) => ({ ...t, args: t.args + event.delta })) ?? next;
    case "tool_call_finish": {
      const call = event.tool_call;
      const args = JSON.stringify(call.arguments);
      const updated = updateTool(next, call.id, (t) => ({ ...t, name: call.name, args }));
      if (updated) return updated;
      const item: ThreadItem = { kind: "tool", id: call.id, name: call.name, args, status: "running" };
      return { ...next, items: [...next.items, item] };
    }
    case "tool_result":
    case "private_tool_result": {
      const status: ToolStatus = event.type === "private_tool_result" ? "private" : event.is_error ? "error" : "done";
      const updated = updateTool(next, event.tool_call_id, (t) => ({ ...t, status, result: event.content }));
      if (updated) return updated;
      const item: ThreadItem = {
        kind: "tool", id: event.tool_call_id, name: event.tool_name, args: "", status, result: event.content,
      };
      return { ...next, items: [...next.items, item] };
    }
    case "permission_request": {
      if (next.pending.some((p) => p.request_id === event.request_id)) return next;
      const { type: _type, ...request } = event;
      return { ...next, pending: [...next.pending, request], running: true };
    }
    case "usage": {
      const { type: _type, ...usage } = event;
      return { ...next, usage };
    }
    case "plan_created":
      return { ...next, plan: event.plan, running: true };
    case "plan_step_updated": {
      if (!next.plan) return next;
      const updatedSteps = next.plan.steps.map((s) =>
        s.id === event.step_id ? { ...s, status: event.status } : s
      );
      return {
        ...next,
        plan: { ...next.plan, steps: updatedSteps },
      };
    }
    case "context_warning":
      return addNotice(next, "warn", event.message);
    case "error":
      return addNotice(next, "error", event.message);
    case "turn_stopped": {
      next = { ...next, running: false, pending: [], stop: { reason: event.reason, message: event.message } };
      // `error` already produced its own notice; completed/cancelled need none.
      if (needsContinue(event.reason) && event.message) next = addNotice(next, "warn", event.message);
      return next;
    }
    case "finish":
    case "provider_item":
    case "subagent_spawned":
    case "subagent_progress":
    case "subagent_finished":
      return next; // per-step bookkeeping; the turn is only over at `turn_stopped`
  }
}

export function resolvePermission(state: ThreadState, requestId: string): ThreadState {
  return { ...state, pending: state.pending.filter((p) => p.request_id !== requestId) };
}

function partsToItems(parts: ContentPart[], role: Message["role"], items: ThreadItem[], nextId: () => string) {
  for (const part of parts) {
    switch (part.type) {
      case "text":
        if (part.text) items.push({ kind: role === "user" ? "user" : "assistant", id: nextId(), text: part.text });
        break;
      case "reasoning":
        if (part.text) items.push({ kind: "reasoning", id: nextId(), text: part.text });
        break;
      case "tool_call":
        items.push({ kind: "tool", id: part.id, name: part.name, args: JSON.stringify(part.arguments), status: "running" });
        break;
      case "tool_result": {
        const tool = items.find((i) => i.kind === "tool" && i.id === part.tool_call_id);
        if (tool && tool.kind === "tool") {
          tool.status = part.is_error ? "error" : "done";
          tool.result = part.content;
        } else {
          items.push({
            kind: "tool", id: part.tool_call_id, name: part.tool_name, args: "",
            status: part.is_error ? "error" : "done", result: part.content,
          });
        }
        break;
      }
      case "provider_item":
        break;
    }
  }
}

/** Build thread items from stored history (a page of `messages`). */
export function itemsFromMessages(messages: Message[], idPrefix = "h"): ThreadItem[] {
  const items: ThreadItem[] = [];
  let n = 0;
  const nextId = () => `${idPrefix}${n++}`;
  for (const m of messages) partsToItems(m.parts, m.role, items, nextId);
  return items;
}

export function threadFromSnapshot(
  messages: Message[],
  opts: {
    running: boolean;
    pending: PermissionRequest[];
    lastSeq: number;
    plan?: Plan | null;
    queue?: string[];
  },
): ThreadState {
  return {
    ...emptyThread(),
    items: itemsFromMessages(messages),
    running: opts.running,
    pending: opts.pending,
    lastSeq: opts.lastSeq,
    plan: opts.plan ?? null,
    queue: opts.queue ?? [],
  };
}
