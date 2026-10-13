//! Post-turn reflection: the agent evaluates its own performance and extracts durable
//! lessons from the completed conversation.
//!
//! This is the core of MauKode's bounded self-improvement loop (inspired by Reflexion +
//! Hermes Agent's learning loop). After a turn completes, the reflection engine:
//!
//! 1. Sends a concise conversation summary to the LLM with a reflection prompt.
//! 2. Extracts lessons (what went well, what went wrong, what to remember).
//! 3. Persists lessons to the memory store as `gotchas` / `conventions`.
//! 4. Optionally proposes a new skill if a reusable pattern is detected.
//!
//! Reflection is opt-in via [`ReflectionConfig`] and never fails the turn.

use crate::context::MemoryBinding;
use crate::skills::SkillManager;
use maukode_llm::{LLMClient, LLMRequest};
use maukode_memory::extract::CandidateMemory;
use maukode_protocol::Message;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, warn};

/// Configuration for the reflection subsystem.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ReflectionConfig {
    /// Master switch. When false, reflection never runs.
    #[serde(default)]
    pub enabled: bool,
    /// Minimum number of tool calls in a turn before reflection triggers. Avoids
    /// reflecting on trivial one-shot turns that yield no lessons.
    #[serde(default = "default_min_tool_calls")]
    pub min_tool_calls: usize,
    /// When true, the engine may write new skill files to `.maukode/skills/learned/`.
    #[serde(default = "default_skill_creation")]
    pub skill_creation: bool,
}

fn default_min_tool_calls() -> usize {
    3
}

fn default_skill_creation() -> bool {
    true
}

impl Default for ReflectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            min_tool_calls: default_min_tool_calls(),
            skill_creation: default_skill_creation(),
        }
    }
}

/// The system prompt sent to the LLM during reflection.
const REFLECTION_SYSTEM_PROMPT: &str = "\
You are a reflection engine for a coding agent. Your job is to analyze the completed \
conversation and extract actionable lessons.\n\n\
Output STRICTLY in this format (no prose outside the blocks):\n\n\
LESSONS:\n\
- [memory: <short title> | <one-sentence lesson, prefixed with taxonomy: gotchas, conventions, or decisions>]\n\
- [memory: ... | ...]\n\
\n\
SKILL:\n\
name: <kebab-case-name>\n\
description: <one-line description>\n\
prompt: <the reusable instruction prompt>\n\
\n\
Rules:\n\
- Only output a SKILL block if you detect a reusable pattern worth codifying.\n\
- If no skill is warranted, output only the LESSONS block.\n\
- Lessons should be specific and actionable, not generic platitudes.\n\
- Use taxonomy 'gotchas' for pitfalls, 'conventions' for best practices, 'decisions' for trade-offs.\n\
- Keep each lesson to one sentence.\n\
- Output at most 3 lessons and at most 1 skill.";

/// Parsed output from the reflection LLM call.
#[derive(Debug, Default, Clone)]
struct ReflectionOutput {
    lessons: Vec<CandidateMemory>,
    skill: Option<ProposedSkill>,
}

/// A skill proposed by the reflection engine, before it is written to disk.
#[derive(Debug, Clone)]
pub struct ProposedSkill {
    pub name: String,
    pub description: String,
    pub prompt: String,
}

/// The reflection engine. Stateless — it borrows the LLM client and memory binding
/// from the session runner.
pub struct ReflectionEngine<'a> {
    llm: &'a Arc<dyn LLMClient>,
    model: &'a str,
    memory: &'a MemoryBinding,
    workspace_dir: &'a Path,
    config: &'a ReflectionConfig,
}

impl<'a> ReflectionEngine<'a> {
    pub fn new(
        llm: &'a Arc<dyn LLMClient>,
        model: &'a str,
        memory: &'a MemoryBinding,
        workspace_dir: &'a Path,
        config: &'a ReflectionConfig,
    ) -> Self {
        Self {
            llm,
            model,
            memory,
            workspace_dir,
            config,
        }
    }

    /// Run the reflection loop on a completed conversation. Never returns an error —
    /// all failures are logged and swallowed so the turn result is unaffected.
    pub async fn reflect(&self, messages: &[Message], tool_call_count: usize) {
        if !self.config.enabled {
            return;
        }
        if tool_call_count < self.config.min_tool_calls {
            debug!(
                "Reflection skipped: only {tool_call_count} tool calls \
                 (min {})",
                self.config.min_tool_calls
            );
            return;
        }

        let summary = build_conversation_summary(messages);
        if summary.is_empty() {
            return;
        }

        let output = match self.call_reflection_llm(&summary).await {
            Ok(o) => o,
            Err(e) => {
                warn!("Reflection LLM call failed: {e}");
                return;
            }
        };

        // 1. Persist lessons to memory store.
        if !output.lessons.is_empty() {
            self.persist_lessons(&output.lessons).await;
        }

        // 2. Write skill file if proposed and skill creation is enabled.
        if let Some(skill) = &output.skill {
            if self.config.skill_creation {
                self.write_skill(skill).await;
            }
        }
    }

    /// Send the conversation summary to the LLM with the reflection prompt and parse
    /// the structured output.
    async fn call_reflection_llm(&self, summary: &str) -> anyhow::Result<ReflectionOutput> {
        let request = LLMRequest::new(
            self.model.to_string(),
            vec![Message::user(format!(
                "Analyze this completed agent conversation and extract lessons.\n\n\
                 --- Conversation Summary ---\n{summary}\n--- End ---"
            ))],
        )
        .with_system_prompt(REFLECTION_SYSTEM_PROMPT)
        .with_max_tokens(1024);

        let mut stream = self.llm.stream(request).await?;
        use futures::StreamExt;
        let mut full_text = String::new();
        while let Some(event) = stream.next().await {
            if let Ok(maukode_protocol::LLMEvent::TextDelta { text }) = event {
                full_text.push_str(&text);
            }
        }

        Ok(parse_reflection_output(&full_text))
    }

    /// Persist extracted lessons to the memory store via the same candidate pipeline
    /// used by auto-extraction.
    async fn persist_lessons(&self, lessons: &[CandidateMemory]) {
        match maukode_memory::persist_candidates(
            self.memory.store(),
            self.memory.scope(),
            lessons,
            self.memory.context.author.as_deref(),
        )
        .await
        {
            Ok(report) => {
                debug!(
                    "Reflection: {} lessons proposed, {} written, {} updated, {} skipped",
                    report.proposed, report.written, report.updated, report.skipped
                );
            }
            Err(e) => warn!("Reflection memory persist failed: {e}"),
        }
    }

    /// Write a proposed skill as a markdown file to `.maukode/skills/learned/`.
    /// The SkillManager will discover it on the next session.
    async fn write_skill(&self, skill: &ProposedSkill) {
        let cwd = self.workspace_dir;
        let skills_dir = cwd.join(".maukode").join("skills").join("learned");
        if let Err(e) = std::fs::create_dir_all(&skills_dir) {
            warn!("Failed to create learned skills dir: {e}");
            return;
        }

        let safe_name = sanitize_skill_name(&skill.name);
        let file_path = skills_dir.join(format!("{safe_name}.md"));
        let content = format!(
            "---\n\
             name: {safe_name}\n\
             description: \"{desc}\"\n\
             run_as_subagent: false\n\
             ---\n\
             {prompt}\n",
            desc = skill.description.replace('"', "\\\""),
            prompt = skill.prompt.trim(),
        );

        // Check if a skill with the same name already exists — if so, append a version
        // suffix rather than overwriting, preserving prior versions for comparison.
        let final_path = if file_path.exists() {
            let stem = format!("{safe_name}-v2");
            skills_dir.join(format!("{stem}.md"))
        } else {
            file_path
        };

        match std::fs::write(&final_path, &content) {
            Ok(()) => {
                debug!(
                    "Reflection: wrote learned skill to {}",
                    final_path.display()
                );
                // Verify the skill parses correctly.
                let parsed =
                    SkillManager::parse_skill_content(&content, &safe_name, Some(&final_path));
                if parsed.name.is_empty() {
                    warn!(
                        "Reflection: skill file at {} failed to parse back correctly",
                        final_path.display()
                    );
                }
            }
            Err(e) => warn!("Failed to write learned skill: {e}"),
        }
    }
}

/// Build a compact summary of the conversation for the reflection prompt.
/// Includes tool calls and their outcomes, truncated to stay within token budget.
fn build_conversation_summary(messages: &[Message]) -> String {
    let mut summary = String::new();
    let mut tool_calls = 0u32;

    for msg in messages {
        let role = match msg.role {
            maukode_protocol::Role::User => "User",
            maukode_protocol::Role::Assistant => "Assistant",
            maukode_protocol::Role::System => continue,
        };

        for part in &msg.parts {
            match part {
                maukode_protocol::ContentPart::Text { text } => {
                    let trimmed = truncate_text(text, 500);
                    summary.push_str(&format!("[{role}]: {trimmed}\n"));
                }
                maukode_protocol::ContentPart::ToolCall(call) => {
                    tool_calls += 1;
                    summary.push_str(&format!("[{role} tool_call #{tool_calls}]: {}", call.name));
                }
                maukode_protocol::ContentPart::ToolResult(result) => {
                    let status = if result.is_error { "ERROR" } else { "OK" };
                    let content = truncate_text(&result.content, 300);
                    summary.push_str(&format!("[Tool result {status}]: {content}\n"));
                }
                _ => {}
            }
        }
    }

    summary
}

/// Truncate text to a max byte length, respecting UTF-8 char boundaries.
fn truncate_text(text: &str, max: usize) -> &str {
    if text.len() <= max {
        text
    } else {
        // Find a safe UTF-8 boundary
        let mut end = max;
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    }
}

/// Sanitize a skill name to a filesystem-safe kebab-case slug.
fn sanitize_skill_name(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Parse the structured reflection output into lessons and an optional skill.
fn parse_reflection_output(raw: &str) -> ReflectionOutput {
    let mut output = ReflectionOutput::default();

    // Extract LESSONS block
    if let Some(lessons_text) = extract_block(raw, "LESSONS:") {
        for line in lessons_text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("- ") {
                if let Some(candidate) = parse_lesson_line(rest) {
                    output.lessons.push(candidate);
                }
            } else if line.starts_with("[memory:") {
                // Handle lines without the "- " prefix
                if let Some(candidate) = parse_lesson_line(line) {
                    output.lessons.push(candidate);
                }
            }
        }
    }

    // Extract SKILL block
    if let Some(skill_text) = extract_block(raw, "SKILL:") {
        if let Some(skill) = parse_skill_block(&skill_text) {
            output.skill = Some(skill);
        }
    }

    output
}

/// Extract the content of a named block from the raw output.
/// Blocks are delimited by "BLOCK_NAME:" followed by content until the next
/// block marker or end of text.
fn extract_block(raw: &str, block_name: &str) -> Option<String> {
    let raw = raw.trim();
    let start = raw.find(block_name)?;
    let content_start = start + block_name.len();

    // Content extends from after the marker to the next marker or end of text.
    let rest = &raw[content_start..];
    let next_marker = rest
        .find("\nLESSONS:")
        .or_else(|| rest.find("\nSKILL:"))
        .or_else(|| rest.find("\n---"));

    let content = match next_marker {
        Some(pos) => &rest[..pos],
        None => rest,
    };

    Some(content.trim().to_string())
}

/// Parse a single `[memory: title | content]` line into a CandidateMemory,
/// extracting taxonomy from the content if prefixed.
fn parse_lesson_line(text: &str) -> Option<CandidateMemory> {
    let text = text.trim();
    let inner = text.strip_prefix("[memory:")?.strip_suffix("]")?;
    let inner = inner.trim();

    let (title, content) = inner.split_once('|')?;
    let title = title.trim().to_string();
    let mut content = content.trim().to_string();

    if title.is_empty() || content.is_empty() {
        return None;
    }

    // Default taxonomy; check if content starts with "taxonomy: X" prefix
    let taxonomy = if let Some(rest) = content.strip_prefix("taxonomy:") {
        let (tax, remaining) = rest
            .split_once(|c: char| c.is_whitespace() || c == ',')
            .unwrap_or((rest, ""));
        let tax = tax.trim().to_string();
        if !tax.is_empty() {
            content = remaining.trim().to_string();
            tax
        } else {
            "gotchas".to_string()
        }
    } else {
        "gotchas".to_string()
    };

    Some(CandidateMemory {
        title,
        content,
        taxonomy,
    })
}

/// Parse a SKILL block into a ProposedSkill.
fn parse_skill_block(text: &str) -> Option<ProposedSkill> {
    let mut name = String::new();
    let mut description = String::new();
    let mut prompt = String::new();
    let mut in_prompt = false;

    for line in text.lines() {
        let line = line.trim();
        if in_prompt {
            prompt.push_str(line);
            prompt.push('\n');
            continue;
        }
        if let Some(rest) = line.strip_prefix("name:") {
            name = rest.trim().to_string();
        } else if let Some(rest) = line.strip_prefix("description:") {
            description = rest.trim().trim_matches('"').to_string();
        } else if let Some(rest) = line.strip_prefix("prompt:") {
            prompt = rest.trim().to_string();
            in_prompt = true;
        }
    }

    if name.is_empty() || prompt.is_empty() {
        return None;
    }

    Some(ProposedSkill {
        description: if description.is_empty() {
            format!("Learned skill: {}", name)
        } else {
            description
        },
        name,
        prompt: prompt.trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lessons_block() {
        let raw = "LESSONS:\n\
                   - [memory: unwrap crash | taxonomy: gotchas, never use unwrap in production paths]\n\
                   - [memory: grep before read | taxonomy: conventions, always grep before read_file to save context]\n";
        let output = parse_reflection_output(raw);
        assert_eq!(output.lessons.len(), 2);
        assert_eq!(output.lessons[0].title, "unwrap crash");
        assert_eq!(output.lessons[0].taxonomy, "gotchas");
        assert_eq!(
            output.lessons[0].content,
            "never use unwrap in production paths"
        );
        assert_eq!(output.lessons[1].taxonomy, "conventions");
        assert!(output.skill.is_none());
    }

    #[test]
    fn parse_skill_block_output() {
        let raw = "LESSONS:\n\
                   - [memory: test lesson | taxonomy: gotchas, a lesson]\n\n\
                   SKILL:\n\
                   name: check-types-first\n\
                   description: \"Verify types before using unwrap\"\n\
                   prompt: Before calling unwrap, check the return type and use \
                   proper error handling.\n";
        let output = parse_reflection_output(raw);
        assert_eq!(output.lessons.len(), 1);
        let skill = output.skill.expect("skill should be parsed");
        assert_eq!(skill.name, "check-types-first");
        assert_eq!(skill.description, "Verify types before using unwrap");
        assert!(skill.prompt.starts_with("Before calling unwrap"));
    }

    #[test]
    fn parse_empty_output() {
        let output = parse_reflection_output("No structured output here.");
        assert!(output.lessons.is_empty());
        assert!(output.skill.is_none());
    }

    #[test]
    fn sanitize_name_handles_special_chars() {
        assert_eq!(sanitize_skill_name("My Cool Skill!"), "my-cool-skill");
        assert_eq!(sanitize_skill_name("already-kebab"), "already-kebab");
        assert_eq!(
            sanitize_skill_name("  spaces everywhere  "),
            "spaces-everywhere"
        );
    }

    #[test]
    fn truncate_text_respects_utf8() {
        let s = "héllo wörld";
        let truncated = truncate_text(s, 5);
        assert!(truncated.len() <= 5);
        assert!(s.starts_with(truncated));
    }

    #[test]
    fn build_summary_includes_tool_calls() {
        let messages = vec![
            Message::user("Fix the bug"),
            Message::assistant(vec![
                maukode_protocol::ContentPart::Text {
                    text: "I'll check the file".to_string(),
                },
                maukode_protocol::ContentPart::ToolCall(maukode_protocol::ToolCall {
                    id: "tc1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::json!({"path": "src/main.rs"}),
                }),
            ]),
        ];
        let summary = build_conversation_summary(&messages);
        assert!(summary.contains("[User]: Fix the bug"));
        assert!(summary.contains("[Assistant tool_call #1]: read_file"));
    }

    #[test]
    fn lesson_without_taxonomy_defaults_to_gotchas() {
        let line = "[memory: simple lesson | this is a lesson]";
        let candidate = parse_lesson_line(line).expect("should parse");
        assert_eq!(candidate.taxonomy, "gotchas");
        assert_eq!(candidate.content, "this is a lesson");
    }
}
