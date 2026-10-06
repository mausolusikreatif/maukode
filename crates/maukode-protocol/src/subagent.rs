use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ModelTier {
    Inherit,
    Fast,
    Strong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SubagentToolMode {
    ReadOnly,
    WorkspaceMutating,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Completed,
    Failed,
    BudgetExceeded,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SubagentProfile {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub role_prompt: String,
    pub tool_mode: SubagentToolMode,
    pub max_turns: usize,
    pub max_tokens: Option<usize>,
    pub model_tier: ModelTier,
}

impl SubagentProfile {
    pub fn codebase_researcher() -> Self {
        Self {
            name: "codebase-researcher".to_string(),
            display_name: "Codebase Researcher".to_string(),
            description: "Deeply researches the codebase, searches symbols, reads files, and analyzes call hierarchies using read-only tools without making any modifications.".to_string(),
            role_prompt: "You are an expert codebase research agent. Your goal is to investigate, search, read, and understand the codebase to answer the user's research objective. You have strictly read-only tools: read_file, glob, grep, and web tools. You cannot edit files or execute bash mutations. Synthesize your findings concisely.".to_string(),
            tool_mode: SubagentToolMode::ReadOnly,
            max_turns: 15,
            max_tokens: Some(32_000),
            model_tier: ModelTier::Fast,
        }
    }

    pub fn security_auditor() -> Self {
        Self {
            name: "security-auditor".to_string(),
            display_name: "Security Auditor".to_string(),
            description: "Audits source files for security vulnerabilities, path traversal risks, secret leaks, and insecure dependencies using read-only inspection tools.".to_string(),
            role_prompt: "You are a specialized security auditor agent. Analyze code for vulnerabilities, potential exploits, insecure data handling, and permission bypasses. You have strictly read-only tools. Report vulnerabilities with severity, affected file lines, and actionable remediation steps.".to_string(),
            tool_mode: SubagentToolMode::ReadOnly,
            max_turns: 10,
            max_tokens: Some(24_000),
            model_tier: ModelTier::Inherit,
        }
    }
}
