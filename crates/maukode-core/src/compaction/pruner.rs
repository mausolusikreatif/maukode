use maukode_protocol::{ContentPart, Message, Role, ToolResult};

/// Context pruner that truncates large tool stdout outputs from past conversation turns
/// to prevent token blowup while maintaining full fidelity in the local session store.
#[derive(Debug, Clone)]
pub struct ContextPruner {
    /// Number of most recent turns where tool outputs are kept at full fidelity
    pub recent_turns_to_keep: usize,
    /// Maximum character length for older tool outputs before head/tail truncation
    pub max_older_output_chars: usize,
    /// Maximum character length for even recent tool outputs (emergency guard against 100MB logs)
    pub emergency_max_chars: usize,
    /// Number of lines to preserve in the head chunk during truncation
    pub head_lines: usize,
    /// Number of lines to preserve in the tail chunk during truncation
    pub tail_lines: usize,
}

impl Default for ContextPruner {
    fn default() -> Self {
        Self {
            recent_turns_to_keep: 2,
            max_older_output_chars: 2000,
            emergency_max_chars: 30000,
            head_lines: 15,
            tail_lines: 15,
        }
    }
}

impl ContextPruner {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_recent_turns(mut self, turns: usize) -> Self {
        self.recent_turns_to_keep = turns;
        self
    }

    pub fn with_max_older_chars(mut self, max_chars: usize) -> Self {
        self.max_older_output_chars = max_chars;
        self
    }

    /// Cap tool outputs longer than `emergency_max_chars`, wherever they are. A message always
    /// comes out the same, so requests built from it keep a stable prefix.
    pub fn cap_messages(&self, messages: &[Message]) -> Vec<Message> {
        messages
            .iter()
            .map(|msg| self.prune_single_message(msg, false))
            .collect()
    }

    /// Prune messages non-destructively for an LLM request payload.
    /// Older tool execution outputs are trimmed with head/tail truncation.
    pub fn prune_messages(&self, messages: &[Message]) -> Vec<Message> {
        if messages.is_empty() {
            return Vec::new();
        }

        // Identify the cutoff index separating older turns from recent turns.
        // In MauKode, a turn is typically marked by an Assistant message or a User message.
        // We count assistant messages from the end of the history.
        let mut assistant_seen = 0;
        let mut cutoff_idx = 0;

        for (idx, msg) in messages.iter().enumerate().rev() {
            if msg.role == Role::Assistant {
                assistant_seen += 1;
                if assistant_seen >= self.recent_turns_to_keep {
                    cutoff_idx = idx;
                    break;
                }
            }
        }

        messages
            .iter()
            .enumerate()
            .map(|(idx, msg)| {
                let is_older_turn = idx < cutoff_idx;
                self.prune_single_message(msg, is_older_turn)
            })
            .collect()
    }

    fn prune_single_message(&self, message: &Message, is_older_turn: bool) -> Message {
        let mut pruned_parts = Vec::with_capacity(message.parts.len());

        for part in &message.parts {
            match part {
                ContentPart::ToolResult(tr) => {
                    let max_allowed = if is_older_turn {
                        self.max_older_output_chars
                    } else {
                        self.emergency_max_chars
                    };

                    if tr.content.len() > max_allowed {
                        let truncated = self.head_tail_truncate(&tr.content, tr.tool_name.as_str());
                        pruned_parts.push(ContentPart::ToolResult(ToolResult {
                            tool_call_id: tr.tool_call_id.clone(),
                            tool_name: tr.tool_name.clone(),
                            content: truncated,
                            is_error: tr.is_error,
                        }));
                    } else {
                        pruned_parts.push(part.clone());
                    }
                }
                _ => pruned_parts.push(part.clone()),
            }
        }

        Message {
            role: message.role,
            parts: pruned_parts,
        }
    }

    /// Truncate text keeping head lines and tail lines with a prominent skip banner
    pub fn head_tail_truncate(&self, content: &str, tool_name: &str) -> String {
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();
        let total_chars = content.len();

        if total_lines <= (self.head_lines + self.tail_lines + 5) {
            // Not enough lines to split into head and tail comfortably, split by byte boundaries
            let half = self.max_older_output_chars / 2;
            let head_end = floor_char_boundary(content, half.min(total_chars));
            let tail_start = ceil_char_boundary(content, total_chars.saturating_sub(half));

            let head = &content[..head_end];
            let tail = if tail_start < total_chars {
                &content[tail_start..]
            } else {
                ""
            };

            let skipped = total_chars.saturating_sub(head.len() + tail.len());
            return format!(
                "{head}\n\n[... Truncated {skipped} characters from {tool_name} output. Raw output preserved in session store ...]\n\n{tail}"
            );
        }

        let head = lines[..self.head_lines].join("\n");
        let tail = lines[total_lines - self.tail_lines..].join("\n");
        let skipped_lines = total_lines - self.head_lines - self.tail_lines;

        format!(
            "{head}\n\n[... Truncated {skipped_lines} lines ({total_chars} chars total) from {tool_name} output. Raw output preserved in session store ...]\n\n{tail}"
        )
    }
}

/// Helper to safely find the closest UTF-8 boundary at or below index
fn floor_char_boundary(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

/// Helper to safely find the closest UTF-8 boundary at or above index
fn ceil_char_boundary(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while index < s.len() && !s.is_char_boundary(index) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;
    use maukode_protocol::ToolResult;

    #[test]
    fn test_short_tool_results_are_not_truncated() {
        let pruner = ContextPruner::default();
        let short_out = "Compiled successfully in 0.42s";
        let msg = Message {
            role: Role::User,
            parts: vec![ContentPart::ToolResult(ToolResult {
                tool_call_id: "call_1".into(),
                tool_name: "bash".into(),
                content: short_out.into(),
                is_error: false,
            })],
        };

        let pruned = pruner.prune_messages(&[msg]);
        assert_eq!(pruned[0].parts.len(), 1);
        if let ContentPart::ToolResult(ref tr) = pruned[0].parts[0] {
            assert_eq!(tr.content, short_out);
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[test]
    fn test_older_tool_results_are_truncated() {
        let pruner = ContextPruner {
            recent_turns_to_keep: 1,
            max_older_output_chars: 200,
            emergency_max_chars: 10000,
            head_lines: 3,
            tail_lines: 3,
        };

        // Create 20 lines of output (over 200 chars)
        let mut long_output = String::new();
        for i in 1..=30 {
            long_output.push_str(&format!("Line {i}: This is some build log output\n"));
        }

        let old_tool_msg = Message {
            role: Role::User,
            parts: vec![ContentPart::ToolResult(ToolResult {
                tool_call_id: "call_1".into(),
                tool_name: "bash".into(),
                content: long_output.clone(),
                is_error: false,
            })],
        };

        let assistant_turn = Message::assistant_text("I have run the build.");
        let recent_user = Message::user("Now run tests");

        let messages = vec![old_tool_msg, assistant_turn, recent_user];
        let pruned = pruner.prune_messages(&messages);

        if let ContentPart::ToolResult(ref tr) = pruned[0].parts[0] {
            assert!(tr.content.contains("Truncated"));
            assert!(tr.content.starts_with("Line 1:"));
            assert!(tr.content.contains("Line 30:"));
            assert!(tr.content.len() < long_output.len());
        } else {
            panic!("Expected ToolResult");
        }
    }

    #[test]
    fn test_recent_turn_within_limit_is_not_truncated() {
        let pruner = ContextPruner {
            recent_turns_to_keep: 2,
            max_older_output_chars: 100,
            emergency_max_chars: 10000,
            head_lines: 2,
            tail_lines: 2,
        };

        let output_150_chars = "x".repeat(150);
        let recent_tool_msg = Message {
            role: Role::User,
            parts: vec![ContentPart::ToolResult(ToolResult {
                tool_call_id: "call_recent".into(),
                tool_name: "read_file".into(),
                content: output_150_chars.clone(),
                is_error: false,
            })],
        };

        let messages = vec![recent_tool_msg];
        let pruned = pruner.prune_messages(&messages);

        if let ContentPart::ToolResult(ref tr) = pruned[0].parts[0] {
            assert_eq!(tr.content, output_150_chars);
        } else {
            panic!("Expected ToolResult");
        }
    }
}
