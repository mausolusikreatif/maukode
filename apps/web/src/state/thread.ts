import { needsContinue } from "../api/types";
import type {
  ContentPart,
  LLMEvent,
  Message,
  PermissionRequest,
  Plan,
  StopReason,
  SessionUsage,
  SubagentStatus,
  TokenUsage,
} from "../api/types";

export type ToolStatus = "running" | "done" | "error" | "private";

export type SubagentRunStatus = "running" | SubagentStatus;

export interface SubagentTraceEntry {
  timestamp: string;
  step: number;
  action: string;
  tokensUsed: number;
}

export interface SubagentDetail {
  id: string;
  parentSessionId: string;
  profileName: string;
  goal: string;
  status: SubagentRunStatus;
  currentStep: number;
  tokensUsed: number;
  summary?: string;
  trace: SubagentTraceEntry[];
}

export type ThreadItem =
  | { kind: "user"; id: string; text: string }
  | { kind: "image"; id: string; mediaType: string; data: string }
  | { kind: "assistant"; id: string; text: string }
  | { kind: "reasoning"; id: string; text: string }
  | { kind: "plan"; id: string; plan: Plan }
  | { kind: "tool"; id: string; name: string; args: string; status: ToolStatus; result?: string }
  | {
      kind: "subagent";
      id: string;
      subagentId: string;
      profileName: string;
      goal: string;
      status: SubagentRunStatus;
      step: number;
      lastAction: string;
      tokensUsed: number;
      summary?: string;
    };

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
  /** The session's tokens and cost so far. */
  sessionUsage: SessionUsage | null;
  /** Active structured plan for the session, if one has been emitted. */
  plan: Plan | null;
  /** Queued prompts waiting for subsequent turns. */
  queue: string[];
  /** Subagent state map keyed by subagent ID */
  subagents: Record<string, SubagentDetail>;
  /** Selected subagent ID whose trace drawer is currently open, if any */
  selectedSubagentId: string | null;
  /** Newest event sequence applied; resume the SSE stream from here. */
  lastSeq: number;
  nextId: number;
}

export function emptyThread(): ThreadState {
  return {
    items: [],
    notices: [],
    running: false,
    pending: [],
    stop: null,
    usage: null,
    sessionUsage: null,
    plan: null,
    queue: [],
    subagents: {},
    selectedSubagentId: null,
    lastSeq: 0,
    nextId: 1,
  };
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
export function startTurn(
  state: ThreadState,
  prompt: string,
  images: { media_type: string; data: string }[] = [],
): ThreadState {
  let next = state;
  for (const image of images) {
    next = pushItem(next, (id) => ({ kind: "image", id, mediaType: image.media_type, data: image.data }));
  }
  next = pushItem(next, (id) => ({ kind: "user", id, text: prompt }));
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
    case "session_usage": {
      const { type: _type, ...sessionUsage } = event;
      return { ...next, sessionUsage };
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
      return next; // per-step bookkeeping; the turn is only over at `turn_stopped`
    case "subagent_spawned": {
      const now = nowTimestamp();
      const detail: SubagentDetail = {
        id: event.subagent_id,
        parentSessionId: event.parent_session_id,
        profileName: event.profile_name,
        goal: event.goal,
        status: "running",
        currentStep: 0,
        tokensUsed: 0,
        trace: [
          {
            timestamp: now,
            step: 0,
            action: `Spawned subagent (${event.profile_name})`,
            tokensUsed: 0,
          },
        ],
      };
      const subagentItem: ThreadItem = {
        kind: "subagent",
        id: `sa_${event.subagent_id}`,
        subagentId: event.subagent_id,
        profileName: event.profile_name,
        goal: event.goal,
        status: "running",
        step: 0,
        lastAction: "Initializing…",
        tokensUsed: 0,
      };
      return {
        ...next,
        subagents: { ...next.subagents, [event.subagent_id]: detail },
        items: [...next.items, subagentItem],
        running: true,
      };
    }
    case "subagent_progress": {
      const existing = next.subagents[event.subagent_id];
      const now = nowTimestamp();
      const traceEntry: SubagentTraceEntry = {
        timestamp: now,
        step: event.step,
        action: event.action,
        tokensUsed: event.tokens_used,
      };
      const updatedDetail: SubagentDetail = existing
        ? {
            ...existing,
            currentStep: event.step,
            tokensUsed: event.tokens_used,
            trace: [...existing.trace, traceEntry],
          }
        : {
            id: event.subagent_id,
            parentSessionId: "",
            profileName: "Subagent",
            goal: "",
            status: "running",
            currentStep: event.step,
            tokensUsed: event.tokens_used,
            trace: [traceEntry],
          };
      const items = next.items.map((i) =>
        i.kind === "subagent" && i.subagentId === event.subagent_id
          ? {
              ...i,
              step: event.step,
              lastAction: event.action,
              tokensUsed: event.tokens_used,
            }
          : i
      );
      return {
        ...next,
        subagents: { ...next.subagents, [event.subagent_id]: updatedDetail },
        items,
        running: true,
      };
    }
    case "subagent_finished": {
      const existing = next.subagents[event.subagent_id];
      const now = nowTimestamp();
      const traceEntry: SubagentTraceEntry = {
        timestamp: now,
        step: (existing?.currentStep ?? 0) + 1,
        action: `Finished (${event.status}): ${event.summary}`,
        tokensUsed: event.tokens_used,
      };
      const updatedDetail: SubagentDetail = existing
        ? {
            ...existing,
            status: event.status,
            tokensUsed: event.tokens_used,
            summary: event.summary,
            trace: [...existing.trace, traceEntry],
          }
        : {
            id: event.subagent_id,
            parentSessionId: "",
            profileName: "Subagent",
            goal: "",
            status: event.status,
            currentStep: 1,
            tokensUsed: event.tokens_used,
            summary: event.summary,
            trace: [traceEntry],
          };
      const items = next.items.map((i) =>
        i.kind === "subagent" && i.subagentId === event.subagent_id
          ? {
              ...i,
              status: event.status,
              summary: event.summary,
              tokensUsed: event.tokens_used,
            }
          : i
      );
      return {
        ...next,
        subagents: { ...next.subagents, [event.subagent_id]: updatedDetail },
        items,
      };
    }
  }
}

function nowTimestamp(): string {
  const d = new Date();
  return `${d.getHours().toString().padStart(2, "0")}:${d.getMinutes().toString().padStart(2, "0")}:${d.getSeconds().toString().padStart(2, "0")}`;
}

export function selectSubagent(state: ThreadState, subagentId: string | null): ThreadState {
  return { ...state, selectedSubagentId: subagentId };
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
      case "image":
        if (role === "user") items.push({ kind: "image", id: nextId(), mediaType: part.media_type, data: part.data });
        break;
      case "provider_item":
      case "context":
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
    subagents?: Record<string, SubagentDetail>;
    selectedSubagentId?: string | null;
    sessionUsage?: SessionUsage | null;
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
    subagents: opts.subagents ?? {},
    selectedSubagentId: opts.selectedSubagentId ?? null,
    sessionUsage: opts.sessionUsage ?? null,
  };
}
