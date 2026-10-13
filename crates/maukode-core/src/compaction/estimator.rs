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
            text.chars().count().div_ceil(4)
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
                    ContentPart::ProviderItem { value } => {
                        total += Self::estimate_text(&value.to_string());
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

    /// Determine context window size (in tokens) for a given model identifier.
    ///
    /// Values are sourced from official provider documentation:
    /// - Anthropic (platform.claude.com/docs/en/models)
    /// - OpenAI (platform.openai.com/docs/models)
    /// - Zhipu/GLM (contextwindows.dev, verified against zhipuai official docs)
    ///
    /// Models are matched by substring of the lowercased identifier.  More
    /// specific patterns are checked before broader ones so that, e.g.,
    /// `claude-opus-4-8` (1M) is not swallowed by the general `claude` branch
    /// (200K for older models).
    pub fn context_window_for_model(model: &str) -> usize {
        let m = model.to_lowercase();

        // ── Anthropic Claude ────────────────────────────────────────────
        //
        // Claude 5.x and Opus 4.7+  → 1M context window
        // Claude 4.x (Opus 4, Sonnet 4/4.5, Haiku 3.5) → 200K
        // Claude 3.x                → 200K
        if m.contains("claude") {
            // Claude 5 family — all have 1M context
            if m.contains("claude-opus-5")
                || m.contains("claude-sonnet-5")
                || m.contains("claude-haiku-5")
                || m.contains("claude-fable-5")
                || m.contains("claude-mythos")
            {
                return 1_000_000;
            }
            // Claude Opus 4.7 and 4.8 — upgraded to 1M
            // Accept both dash (claude-opus-4-7) and dot (claude-opus-4.7) separators
            // since gateways like llmkita use dotted version notation.
            if m.contains("claude-opus-4")
                && (m.contains("4-7")
                    || m.contains("4.7")
                    || m.contains("4-8")
                    || m.contains("4.8"))
            {
                return 1_000_000;
            }
            // All other Claude models (Opus 4/4.5, Sonnet 4/4.5, Haiku 3.5, 3.x)
            return 200_000;
        }

        // ── OpenAI GPT ──────────────────────────────────────────────────
        //
        // GPT-6 family       → 1,050,000 (922K input + 128K output)
        // GPT-5.4 / 5.6 family → 1,050,000 (922K input + 128K output)
        // GPT-5 / GPT-5.1    → 400,000 (272K input + 128K output)
        // GPT-4o             → 128,000
        // GPT-4 (non-turbo)  → 8,192
        // GPT-4-32k          → 32,768
        // GPT-3.5            → 16,384
        if m.contains("gpt") {
            // GPT-6 family
            if m.contains("gpt-6") {
                return 1_050_000;
            }
            // GPT-5.4 and GPT-5.6 families — 1M+ context
            if m.contains("gpt-5-4")
                || m.contains("gpt-5.4")
                || m.contains("gpt-5-6")
                || m.contains("gpt-5.6")
            {
                return 1_050_000;
            }
            // GPT-5 and GPT-5.1 — 400K context (272K input + 128K output)
            if m.contains("gpt-5") {
                return 400_000;
            }
            // GPT-4o — 128K context
            if m.contains("gpt-4o") {
                return 128_000;
            }
            // GPT-4-32k variant
            if m.contains("gpt-4-32k") {
                return 32_768;
            }
            // GPT-4 non-turbo base models — 8K
            if m.contains("gpt-4")
                && !m.contains("turbo")
                && !m.contains("1106")
                && !m.contains("0125")
            {
                return 8_192;
            }
            // GPT-3.5 — 16K
            if m.contains("gpt-3.5") {
                return 16_384;
            }
        }

        // ── Zhipu GLM ───────────────────────────────────────────────────
        //
        // GLM-5.x   → 1,000,000
        // GLM-4.5   → 131,072 (128K)
        // GLM-4     → 131,072 (128K)
        if m.contains("glm") {
            if m.contains("glm-5") || m.contains("glm5") {
                return 1_000_000;
            }
            // GLM-4.x and GLM-latest variants
            return 131_072;
        }

        // ── Google Gemini ───────────────────────────────────────────────
        if m.contains("gemini") {
            return 1_000_000;
        }

        // ── DeepSeek ────────────────────────────────────────────────────
        if m.contains("deepseek") {
            return 64_000;
        }

        // ── Qwen ────────────────────────────────────────────────────────
        if m.contains("qwen") {
            return 32_768;
        }

        // ── Default fallback ────────────────────────────────────────────
        128_000
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
        // Claude
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-3-7-sonnet"),
            200_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-sonnet-4-5"),
            200_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-opus-4-1"),
            200_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-opus-4-8"),
            1_000_000
        );
        // Dotted notation (used by llmkita gateway: "claude-opus-4.8")
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-opus-4.8"),
            1_000_000
        );
        // Opus 4.1 must NOT match the 4.7/4.8 1M rule
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-opus-4-1"),
            200_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-haiku-5-5"),
            1_000_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-sonnet-5-5"),
            1_000_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("claude-opus-4.8"),
            1_000_000
        );

        // GPT
        assert_eq!(TokenEstimator::context_window_for_model("gpt-4o"), 128_000);
        assert_eq!(TokenEstimator::context_window_for_model("gpt-5"), 400_000);
        assert_eq!(TokenEstimator::context_window_for_model("gpt-5.1"), 400_000);
        assert_eq!(
            TokenEstimator::context_window_for_model("gpt-5.6"),
            1_050_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("gpt-5.6-luna"),
            1_050_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("gpt-6-astra"),
            1_050_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("gpt-6-luna"),
            1_050_000
        );

        // GLM
        assert_eq!(TokenEstimator::context_window_for_model("glm-4.5"), 131_072);
        assert_eq!(
            TokenEstimator::context_window_for_model("glm-5.2"),
            1_000_000
        );
        assert_eq!(
            TokenEstimator::context_window_for_model("glm-5.3-flash"),
            1_000_000
        );

        // Gemini
        assert_eq!(
            TokenEstimator::context_window_for_model("gemini-1.5-pro"),
            1_000_000
        );

        // DeepSeek
        assert_eq!(
            TokenEstimator::context_window_for_model("deepseek-coder"),
            64_000
        );

        // Unknown model → fallback
        assert_eq!(
            TokenEstimator::context_window_for_model("some-unknown-model"),
            128_000
        );
    }

    #[test]
    fn test_threshold_checking() {
        let msgs = vec![
            Message::user("Hello assistant, please help me build a website."),
            Message::assistant_text("Sure, what kind of website?"),
        ];

        let usage =
            TokenEstimator::check_threshold(&msgs, "System instructions", "claude-3-7-sonnet");
        assert!(!usage.exceeds_threshold);
        assert!(!usage.is_critical);
        assert!(usage.estimated_tokens > 0);
        assert_eq!(usage.context_limit, 200_000);
    }
}
