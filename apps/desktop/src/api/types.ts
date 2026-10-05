// Re-exports of the Rust-generated types (see api/generated, produced by `cargo test --features ts`).
export type { AuditResponse } from "./generated/AuditResponse";
export type { Checkpoint } from "./generated/Checkpoint";
export type { ContentPart } from "./generated/ContentPart";
export type { CreateSessionRequest } from "./generated/CreateSessionRequest";
export type { CreateSessionResponse } from "./generated/CreateSessionResponse";
export type { DiffResponse } from "./generated/DiffResponse";
export type { FileDiffEntry } from "./generated/FileDiffEntry";
export type { FileDiffListResponse } from "./generated/FileDiffListResponse";
export type { FileDiffResponse } from "./generated/FileDiffResponse";
export type { GlobalEvent } from "./generated/GlobalEvent";
export type { LLMEvent } from "./generated/LLMEvent";
export type { MergeOutcome } from "./generated/MergeOutcome";
export type { Message } from "./generated/Message";
export type { MessagesPage } from "./generated/MessagesPage";
export type { ModelsListResponse } from "./generated/ModelsListResponse";
export type { PermissionDecision } from "./generated/PermissionDecision";
export type { PermissionMode } from "./generated/PermissionMode";
export type { PermissionRequest } from "./generated/PermissionRequest";
export type { ProvidersListResponse } from "./generated/ProvidersListResponse";
export type { SessionDetailResponse } from "./generated/SessionDetailResponse";
export type { SessionListItem } from "./generated/SessionListItem";
export type { StopReason } from "./generated/StopReason";
export type { TokenUsage } from "./generated/TokenUsage";
export type { UndoResponse } from "./generated/UndoResponse";
export type { WorktreeStatus } from "./generated/WorktreeStatus";

/** True when the user probably needs to send "continue" to finish the task. */
export function needsContinue(reason: import("./generated/StopReason").StopReason): boolean {
  return reason === "max_tokens" || reason === "max_steps";
}
