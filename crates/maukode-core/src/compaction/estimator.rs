use maukode_protocol::{ContentPart, Message};

#[derive(Debug, Clone, PartialEq)]
pub struct ContextUsage {
    pub estimated_tokens: usize,
    pub context_limit: usize,
    pub ratio: f32,
    pub exceeds_threshold: bool,
    pub is_critical: bool,
}

pub struct TokenEstimator;

impl TokenEstimator {
    /// Estimate token count for arbitrary string using standard ~4 chars/token heuristic
    pub fn estimate_text(text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            (text.chars().count() + 3) / 4
        }
    }

    /// Estimate total tokens for a conversation history including system prompt and tool payloads
    pub fn estimate_messages(messages: &[Message], system_prompt: &str) -> usize {
        let mut total = Self::estimate_text(system_prompt);

        for msg in messages {
            // Per-message framing overhead (~4 tokens for role tags & delimiters)
            total += 4;

            for part in &msg.parts {
                match part {
                    ContentPart::Text { text } => {
                        total += Self::estimate_text(text);
                    }
                    ContentPart::Reasoning { text } => {
                        total += Self::estimate_text(text);
                    }
                    ContentPart::ToolCall(call) => {
                        total += Self::estimate_text(&call.name);
                        total += Self::estimate_text(&call.arguments.to_string());
                        total += 4; // tool framing
                    }
                    ContentPart::ToolResult(res) => {
                        total += Self::estimate_text(&res.tool_name);
                        total += Self::estimate_text(&res.content);
                        total += 4; // result framing
                    }
                }
            }
        }

        total
    }

    /// Determine context window size (in tokens) for a given model identifier
    pub fn context_window_for_model(model: &str) -> usize {
        let m = model.to_lowercase();

        if m.contains("claude") {
            200_000
        } else if m.contains("gemini") {
            1_000_000
        } else if m.contains("gpt-4o") || m.contains("o1") || m.contains("o3") {
            128_000
        } else if m.contains("gpt-4-turbo") || m.contains("gpt-4-1106") || m.contains("gpt-4-0125") {
            128_000
        } else if m.contains("gpt-4-32k") {
            32_768
        } else if m.contains("gpt-4") {
            8_192
        } else if m.contains("gpt-3.5-turbo-16k") {
            16_384
        } else if m.contains("gpt-3.5") {
            16_384
        } else if m.contains("deepseek") {
            64_000
        } else if m.contains("qwen") {
            32_768
        } else if m.contains("llama-3.1") || m.contains("llama-3.2") || m.contains("llama-3.3") {
            128_000
        } else {
            128_000
        }
    }

    /// Check conversation token usage against model context limit
    /// Default warning threshold is 75%, critical threshold is 85%.
    pub fn check_threshold(messages: &[Message], system_prompt: &str, model: &str) -> ContextUsage {
        let estimated_tokens = Self::estimate_messages(messages, system_prompt);
        let context_limit = Self::context_window_for_model(model);
        let ratio = if context_limit > 0 {
            (estimated_tokens as f32) / (context_limit as f32)
        } else {
            0.0
        };

        ContextUsage {
            estimated_tokens,
            context_limit,
            ratio,
            exceeds_threshold: ratio >= 0.75,
            is_critical: ratio >= 0.85,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_estimate_calculation() {
        assert_eq!(TokenEstimator::estimate_text(""), 0);
        assert_eq!(TokenEstimator::estimate_text("test"), 1);
        assert_eq!(TokenEstimator::estimate_text("hello world!"), 3);
    }

    #[test]
    fn test_context_window_lookup() {
        assert_eq!(TokenEstimator::context_window_for_model("claude-3-7-sonnet"), 200_000);
        assert_eq!(TokenEstimator::context_window_for_model("gpt-4o"), 128_000);
        assert_eq!(TokenEstimator::context_window_for_model("gemini-1.5-pro"), 1_000_000);
        assert_eq!(TokenEstimator::context_window_for_model("deepseek-coder"), 64_000);
    }

    #[test]
    fn test_threshold_checking() {
        let msgs = vec![
            Message::user("Hello assistant, please help me build a website."),
            Message::assistant_text("Sure, what kind of website?"),
        ];

        let usage = TokenEstimator::check_threshold(&msgs, "System instructions", "claude-3-7-sonnet");
        assert!(!usage.exceeds_threshold);
        assert!(!usage.is_critical);
        assert!(usage.estimated_tokens > 0);
        assert_eq!(usage.context_limit, 200_000);
    }
}
