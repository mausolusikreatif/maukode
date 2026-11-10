// Re-exports of the Rust-generated types (see api/generated, produced by `cargo test --features ts`).
export type { AuditResponse } from "./generated/AuditResponse";
export type { AgentConfig } from "./generated/AgentConfig";
export type { Checkpoint } from "./generated/Checkpoint";
export type { ChatGptAccount } from "./generated/ChatGptAccount";
export type { ChatGptLoginStarted } from "./generated/ChatGptLoginStarted";
export type { ChatGptLoginStatus } from "./generated/ChatGptLoginStatus";
export type { ChatGptStatus } from "./generated/ChatGptStatus";
export type { CodebaseMapResponse } from "./generated/CodebaseMapResponse";
export type { ContentPart } from "./generated/ContentPart";
export type { CreateSessionRequest } from "./generated/CreateSessionRequest";
export type { CreateSessionResponse } from "./generated/CreateSessionResponse";
export type { DiffResponse } from "./generated/DiffResponse";
export type { FileDiffEntry } from "./generated/FileDiffEntry";
export type { FileDiffListResponse } from "./generated/FileDiffListResponse";
export type { FileDiffResponse } from "./generated/FileDiffResponse";
export type { GlobalEvent } from "./generated/GlobalEvent";
export type { LLMEvent } from "./generated/LLMEvent";
export type { McpListResponse } from "./generated/McpListResponse";
export type { MergeOutcome } from "./generated/MergeOutcome";
export type { Message } from "./generated/Message";
export type { MessagesPage } from "./generated/MessagesPage";
export type { ModelsListResponse } from "./generated/ModelsListResponse";
export type { ModelTier } from "./generated/ModelTier";
export type { PermissionDecision } from "./generated/PermissionDecision";
export type { PermissionMode } from "./generated/PermissionMode";
export type { PermissionRequest } from "./generated/PermissionRequest";
export type { Plan } from "./generated/Plan";
export type { PlanStep } from "./generated/PlanStep";
export type { PluginManifest } from "./generated/PluginManifest";
export type { PreviewInfo } from "./generated/PreviewInfo";
export type { PreviewRequest } from "./generated/PreviewRequest";
export type { ProviderConfig } from "./generated/ProviderConfig";
export type { ProviderKind } from "./generated/ProviderKind";
export type { ProvidersListResponse } from "./generated/ProvidersListResponse";
export type { QueueSteeringPrompt } from "./generated/QueueSteeringPrompt";
export type { ConsolidationConfig } from "./generated/ConsolidationConfig";
export type { ReflectionConfig } from "./generated/ReflectionConfig";
export type { ExecutionSettings } from "./generated/ExecutionSettings";
export type { ExecutionStatus } from "./generated/ExecutionStatus";
export type { LawangRuntimeStatus } from "./generated/LawangRuntimeStatus";
export type { LawangSettingsView } from "./generated/LawangSettingsView";
export type { SetExecutionRequest } from "./generated/SetExecutionRequest";
export type { SetLawangRequest } from "./generated/SetLawangRequest";
export type { SecuritySettings } from "./generated/SecuritySettings";
export type { SandboxMode } from "./generated/SandboxMode";
export type { SessionDetailResponse } from "./generated/SessionDetailResponse";
export type { SessionUsage } from "./generated/SessionUsage";
export type { PromptImage } from "./generated/PromptImage";
export type { SessionListItem } from "./generated/SessionListItem";
export type { SetAgentConfigRequest } from "./generated/SetAgentConfigRequest";
export type { SkillDefinition } from "./generated/SkillDefinition";
export type { StepStatus } from "./generated/StepStatus";
export type { StopReason } from "./generated/StopReason";
export type { SubagentProfile } from "./generated/SubagentProfile";
export type { SubagentStatus } from "./generated/SubagentStatus";
export type { SubagentToolMode } from "./generated/SubagentToolMode";
export type { TokenUsage } from "./generated/TokenUsage";
export type { UndoResponse } from "./generated/UndoResponse";
export type { WorktreeStatus } from "./generated/WorktreeStatus";

/** `GET /api/v1/sessions/{id}/system-prompt`: written by hand, the server builds it without ts-rs. */
export interface SystemPromptInfo {
  session_id: string;
  hash: string;
  created_at: string;
  system_prompt: string;
  system_prompt_tokens: number;
  tool_schema_tokens: number;
  tools: { name: string; tokens: number }[];
}

/** True when the user probably needs to send "continue" to finish the task. */
export function needsContinue(reason: import("./generated/StopReason").StopReason): boolean {
  return reason === "max_tokens" || reason === "max_steps";
}
