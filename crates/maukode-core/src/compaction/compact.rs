use crate::compaction::estimator::TokenEstimator;
use crate::session::Session;
use anyhow::Result;
use futures::StreamExt;
use maukode_llm::{LLMClient, LLMRequest};
use maukode_protocol::{ContentPart, LLMEvent, Message, Role};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub original_message_count: usize,
    pub compacted_message_count: usize,
    pub original_tokens: usize,
    pub compacted_tokens: usize,
    pub summary: String,
    pub archived_messages: Vec<Message>,
}

pub struct ConversationCompactor;

fn without_thinking_blocks(message: &Message) -> Message {
    Message {
        role: message.role,
        parts: message
            .parts
            .iter()
            .filter(|part| {
                !matches!(part, ContentPart::ProviderItem { value }
                    if matches!(value["type"].as_str(), Some("thinking" | "redacted_thinking")))
            })
            .cloned()
            .collect(),
    }
}

impl ConversationCompactor {
    /// Compact an active session in-place.
    /// Returns the compaction statistics, summary, and the archived full-fidelity messages.
    pub async fn compact(
        session: &mut Session,
        llm: Option<&dyn LLMClient>,
        model: &str,
        focus: Option<&str>,
    ) -> Result<CompactionResult> {
        let total_messages = session.messages.len();
        let original_tokens = TokenEstimator::estimate_messages(&session.messages, "");

        if total_messages <= 4 {
            let summary =
                "Session history is already compact (<= 4 messages). No compaction needed."
                    .to_string();
            return Ok(CompactionResult {
                original_message_count: total_messages,
                compacted_message_count: total_messages,
                original_tokens,
                compacted_tokens: original_tokens,
                summary,
                archived_messages: session.messages.clone(),
            });
        }

        let archived_messages = session.messages.clone();

        // Keep the most recent turns (last 2-4 messages)
        let keep_count = if total_messages <= 6 { 2 } else { 4 };
        let split_idx = total_messages.saturating_sub(keep_count);
        let older_messages = &session.messages[..split_idx];
        let recent_messages = &session.messages[split_idx..];

        // Generate summary: try LLM if available, else heuristic fallback
        let summary = if let Some(client) = llm {
            match Self::summarize_with_llm(client, model, older_messages, focus).await {
                Ok(s) if !s.trim().is_empty() => s,
                Ok(_) | Err(_) => Self::heuristic_summary(older_messages, focus),
            }
        } else {
            Self::heuristic_summary(older_messages, focus)
        };

        // Construct compaction anchor messages ensuring strict User/Assistant alternation
        let anchor_user_content = format!(
            "[CONVERSATION SUMMARY - COMPACTED CONTEXT]\n\
             The earlier part of this conversation ({} turns) was compacted to preserve context space.\n\
             Summary of objectives, files modified, and decisions:\n\n\
             {}\n\n\
             [Continue session from this state.]",
            older_messages.len(),
            summary.trim()
        );
        let anchor_user = Message::user(anchor_user_content);

        let mut new_messages = Vec::new();
        new_messages.push(anchor_user);

        let next_role = recent_messages.first().map(|m| m.role);
        if next_role == Some(Role::User) || next_role.is_none() {
            new_messages.push(Message::assistant_text(
                "Understood. I have integrated the previous conversation summary and am ready to proceed.",
            ));
        }

        // Anthropic thinking blocks are bound to the history before them, which is replaced here;
        // replayed after the summary they would be dropped (or rejected) by the provider.
        new_messages.extend(recent_messages.iter().map(without_thinking_blocks));

        let compacted_tokens = TokenEstimator::estimate_messages(&new_messages, "");
        let compacted_message_count = new_messages.len();

        session.messages = new_messages;
        session.updated_at = chrono::Utc::now();
        // The cached prefix starts over anyway: the next request builds a fresh system prompt.
        session.system_prompt = None;

        Ok(CompactionResult {
            original_message_count: total_messages,
            compacted_message_count,
            original_tokens,
            compacted_tokens,
            summary,
            archived_messages,
        })
    }

    /// LLM-powered summarizer
    async fn summarize_with_llm(
        client: &dyn LLMClient,
        model: &str,
        messages: &[Message],
        focus: Option<&str>,
    ) -> Result<String> {
        let focus_instruction = focus
            .map(|f| format!("\nAdditional User Focus Directive: {f}"))
            .unwrap_or_default();

        let system_prompt = format!(
            "You are a technical AI agent performing conversation compaction for a coding session.\n\
             Analyze the provided conversation history and write a structured, high-density summary.\n\
             You MUST format the summary with the following sections:\n\
             1. Key Goals & User Objectives\n\
             2. Files Created, Inspected, or Modified\n\
             3. Key Decisions Made & Solutions Adopted\n\
             4. Current Status & Remaining Open Tasks\n\
             Be specific about file paths and technical details.{focus_instruction}"
        );

        let mut req_messages = messages.to_vec();
        req_messages.push(Message::user(
            "Please summarize our conversation so far according to the specified structure.",
        ));

        let req = LLMRequest::new(model, req_messages).with_system_prompt(&system_prompt);

        let mut stream = client.stream(req).await?;
        let mut out = String::new();

        while let Some(event_res) = stream.next().await {
            if let Ok(LLMEvent::TextDelta { text }) = event_res {
                out.push_str(&text);
            }
        }

        Ok(out)
    }

    /// Fast and robust heuristic summarization when LLM is unavailable or offline
    pub fn heuristic_summary(messages: &[Message], focus: Option<&str>) -> String {
        let mut user_prompts = Vec::new();
        let mut files_modified = HashSet::new();
        let mut files_inspected = HashSet::new();
        let mut commands_run = Vec::new();
        let mut latest_assistant_conclusions = Vec::new();

        for msg in messages {
            match msg.role {
                Role::User => {
                    let text = msg.text_content().trim().to_string();
                    if !text.is_empty() && !text.starts_with('[') {
                        user_prompts.push(text);
                    }
                }
                Role::Assistant => {
                    let mut summary_line = String::new();
                    for part in &msg.parts {
                        match part {
                            ContentPart::Text { text } => {
                                let trimmed = text.trim();
                                if !trimmed.is_empty() {
                                    let first_line = trimmed.lines().next().unwrap_or("");
                                    if first_line.len() > 10 {
                                        summary_line = first_line.to_string();
                                    }
                                }
                            }
                            ContentPart::ToolCall(call) => match call.name.as_str() {
                                "write_to_file" | "replace_file_content" | "edit_file" => {
                                    if let Some(target) = call
                                        .arguments
                                        .get("TargetFile")
                                        .or_else(|| call.arguments.get("path"))
                                        .and_then(|v| v.as_str())
                                    {
                                        files_modified.insert(target.to_string());
                                    }
                                }
                                "view_file" | "read_file" => {
                                    if let Some(target) = call
                                        .arguments
                                        .get("AbsolutePath")
                                        .or_else(|| call.arguments.get("path"))
                                        .and_then(|v| v.as_str())
                                    {
                                        files_inspected.insert(target.to_string());
                                    }
                                }
                                "bash" => {
                                    if let Some(cmd) = call
                                        .arguments
                                        .get("command")
                                        .or_else(|| call.arguments.get("CommandLine"))
                                        .and_then(|v| v.as_str())
                                    {
                                        commands_run.push(cmd.to_string());
                                    }
                                }
                                "web_search" | "search_web" => {
                                    if let Some(q) =
                                        call.arguments.get("query").and_then(|v| v.as_str())
                                    {
                                        commands_run.push(format!("web_search: \"{q}\""));
                                    }
                                }
                                "fetch_web_content" | "fetch_url" | "read_url" => {
                                    if let Some(u) =
                                        call.arguments.get("url").and_then(|v| v.as_str())
                                    {
                                        commands_run.push(format!("fetch_url: {u}"));
                                    }
                                }
                                _ => {}
                            },
                            _ => {}
                        }
                    }
                    if !summary_line.is_empty() {
                        latest_assistant_conclusions.push(summary_line);
                    }
                }
                _ => {}
            }
        }

        let mut lines = Vec::new();

        if let Some(f) = focus {
            lines.push(format!("**Focus Area:** {f}\n"));
        }

        lines.push("#### 1. Key Objectives & User Prompts".to_string());
        if user_prompts.is_empty() {
            lines.push("- Initial exploratory task".to_string());
        } else {
            for p in user_prompts.iter().take(5) {
                let preview = if p.len() > 100 {
                    let end = p.floor_char_boundary(100);
                    format!("{}...", &p[..end])
                } else {
                    p.clone()
                };
                lines.push(format!("- {preview}"));
            }
        }

        lines.push("\n#### 2. Modified & Inspected Files".to_string());
        if files_modified.is_empty() && files_inspected.is_empty() {
            lines.push("- None recorded in earlier turns".to_string());
        } else {
            for f in &files_modified {
                lines.push(format!("- ✏️ Modified: `{f}`"));
            }
            for f in files_inspected.iter().take(10) {
                if !files_modified.contains(f) {
                    lines.push(format!("- 🔍 Read: `{f}`"));
                }
            }
        }

        lines.push("\n#### 3. Key Actions & Commands".to_string());
        if commands_run.is_empty() {
            lines.push("- No shell commands executed in earlier turns".to_string());
        } else {
            for cmd in commands_run.iter().rev().take(5).rev() {
                lines.push(format!("- `$ {cmd}`"));
            }
        }

        if !latest_assistant_conclusions.is_empty() {
            lines.push("\n#### 4. Decisions & Milestones".to_string());
            for c in latest_assistant_conclusions.iter().rev().take(3).rev() {
                lines.push(format!("- {c}"));
            }
        }

        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maukode_protocol::ToolCall;

    #[tokio::test]
    async fn compaction_drops_thinking_blocks_from_kept_messages() {
        let mut session = Session::new(std::env::temp_dir());
        session.system_prompt = Some("old prompt".into());
        for i in 0..8 {
            session.add_message(Message::user(format!("question {i}")));
            session.add_message(Message::assistant(vec![
                ContentPart::ProviderItem {
                    value: serde_json::json!({"type": "thinking", "thinking": "t", "signature": "s"}),
                },
                ContentPart::ProviderItem {
                    value: serde_json::json!({"type": "reasoning", "encrypted_content": "e"}),
                },
                ContentPart::Text {
                    text: format!("answer {i}"),
                },
            ]));
        }

        ConversationCompactor::compact(&mut session, None, "model", None)
            .await
            .unwrap();

        assert!(session.system_prompt.is_none());
        let kept: Vec<_> = session.messages.iter().flat_map(|m| &m.parts).collect();
        assert!(kept
            .iter()
            .any(|p| matches!(p, ContentPart::Text { text } if text == "answer 7")));
        assert!(kept
            .iter()
            .all(|p| !matches!(p, ContentPart::ProviderItem { value }
            if value["type"] == "thinking")));
        // Other providers' items are not bound to the history and stay.
        assert!(kept
            .iter()
            .any(|p| matches!(p, ContentPart::ProviderItem { value }
            if value["type"] == "reasoning")));
    }

    #[tokio::test]
    async fn test_heuristic_summary_extraction() {
        let msg1 = Message::user("Please build the database and run tests");
        let msg2 = Message::assistant(vec![
            ContentPart::ToolCall(ToolCall {
                id: "1".into(),
                name: "write_to_file".into(),
                arguments: serde_json::json!({ "TargetFile": "/path/to/db.rs" }),
            }),
            ContentPart::ToolCall(ToolCall {
                id: "2".into(),
                name: "bash".into(),
                arguments: serde_json::json!({ "command": "cargo test" }),
            }),
            ContentPart::Text {
                text: "Created the database schema and ran test suite successfully.".into(),
            },
        ]);

        let summary = ConversationCompactor::heuristic_summary(&[msg1, msg2], None);
        assert!(summary.contains("Please build the database"));
        assert!(summary.contains("/path/to/db.rs"));
        assert!(summary.contains("cargo test"));
    }

    #[tokio::test]
    async fn test_compaction_reduces_messages() {
        let mut session = Session::new(".");
        for i in 1..=10 {
            session.add_message(Message::user(format!(
                "Step {i} instructions: Please inspect the architecture and implement component #{i} with full unit tests and comprehensive error handling."
            )));
            session.add_message(Message::assistant_text(format!(
                "Executed step {i}. Created necessary files and modules. Added unit tests with 100% coverage. Verified no regressions in existing codebase."
            )));
        }

        assert_eq!(session.messages.len(), 20);

        let res = ConversationCompactor::compact(&mut session, None, "claude-3-7-sonnet", None)
            .await
            .expect("Compaction should succeed");

        assert_eq!(res.original_message_count, 20);
        assert!(res.compacted_message_count < 10);
        assert!(session.messages.len() < 10);
        assert!(res.compacted_tokens < res.original_tokens);
        assert_eq!(res.archived_messages.len(), 20);
    }
}
