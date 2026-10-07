use crate::subagent::{ModelTier, SubagentProfile, SubagentToolMode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SkillDefinition {
    pub name: String,
    pub description: String,
    pub prompt: String,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub is_built_in: bool,
    #[serde(default)]
    pub run_as_subagent: bool,
}

impl SkillDefinition {
    pub fn review_pr() -> Self {
        Self {
            name: "review-pr".to_string(),
            description: "Review pull request or git diff for potential bugs, security issues, performance regressions, and code quality.".to_string(),
            prompt: "You are an expert code reviewer. Analyze the current git changes or pull request branch carefully. Inspect the modified files, check for logic bugs, edge cases, performance regressions, security vulnerabilities, and adherence to repository guidelines and conventions. Provide structured feedback grouped by severity.".to_string(),
            allowed_tools: Some(vec![
                "read_file".to_string(),
                "glob".to_string(),
                "grep".to_string(),
                "diff_tool".to_string(),
                "bash".to_string(),
            ]),
            path: None,
            is_built_in: true,
            run_as_subagent: false,
        }
    }

    pub fn fix_ci() -> Self {
        Self {
            name: "fix-ci".to_string(),
            description: "Diagnose and fix broken CI pipelines, compilation errors, and failing test suites.".to_string(),
            prompt: "You are a CI and test repair specialist. Examine the failed CI logs or test errors, locate the failing code, determine the minimal root cause, and apply surgical fixes to get all tests passing without breaking unrelated functionality.".to_string(),
            allowed_tools: None,
            path: None,
            is_built_in: true,
            run_as_subagent: false,
        }
    }

    pub fn security_audit() -> Self {
        Self {
            name: "security-audit".to_string(),
            description: "Audit the codebase for vulnerabilities, sensitive data exposure, and security risks.".to_string(),
            prompt: "You are a specialized security auditor agent. Analyze source code and configuration for security vulnerabilities, injection flaws, path traversal, sensitive credential leaks, and insecure dependencies. Report findings with severity, proof-of-concept explanation, and remediation guidance.".to_string(),
            allowed_tools: Some(vec![
                "read_file".to_string(),
                "glob".to_string(),
                "grep".to_string(),
            ]),
            path: None,
            is_built_in: true,
            run_as_subagent: true,
        }
    }

    pub fn built_in_skills() -> Vec<Self> {
        vec![Self::review_pr(), Self::fix_ci(), Self::security_audit()]
    }

    pub fn to_subagent_profile(&self) -> SubagentProfile {
        let display_name = self
            .name
            .split(['-', '_'])
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" ");

        let is_read_only = self.allowed_tools.as_ref().is_some_and(|tools| {
            !tools
                .iter()
                .any(|t| t == "write_file" || t == "edit_file" || t == "bash")
        });

        SubagentProfile {
            name: self.name.clone(),
            display_name,
            description: self.description.clone(),
            role_prompt: self.prompt.clone(),
            tool_mode: if is_read_only {
                SubagentToolMode::ReadOnly
            } else {
                SubagentToolMode::WorkspaceMutating
            },
            max_turns: 15,
            max_tokens: Some(32_000),
            model_tier: ModelTier::Inherit,
            allowed_mcp_servers: None,
            allowed_tools: self.allowed_tools.clone(),
        }
    }
}
