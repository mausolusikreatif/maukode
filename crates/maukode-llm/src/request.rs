use maukode_protocol::Message;
use maukode_tools::ToolDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    /// Extended thinking, for providers that support it (Anthropic).
    #[serde(default)]
    pub thinking: ThinkingSettings,
}

impl LLMRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            system_prompt: None,
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(4096),
            thinking: ThinkingSettings::default(),
        }
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_tools(mut self, tools: Vec<ToolDefinition>) -> Self {
        self.tools = tools;
        self
    }

    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }
}

/// How a [`maukode_protocol::ContentPart::Context`] part is shown to the model.
pub fn context_text(context: &str) -> String {
    format!("<maukode-context>\n{}\n</maukode-context>", context.trim())
}

/// When to ask the model to think before answering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    /// Adaptive thinking on models that support it (Claude 4.6 and later), none on older ones.
    #[default]
    Auto,
    /// The model decides when and how much to think.
    Adaptive,
    /// A fixed thinking budget (`budget_tokens`), for models older than Claude 4.6.
    Budget,
    /// Do not request thinking. Models that always think (Claude 5 and later) still do.
    Off,
}

/// Extended thinking settings for one model.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingSettings {
    #[serde(default)]
    pub mode: ThinkingMode,
    /// Thinking budget for [`ThinkingMode::Budget`] (at least 1024).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_tokens: Option<u32>,
    /// `low`, `medium`, `high`, `xhigh` or `max`; the model's default when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Return a readable summary of the thinking (`true`, the default) or empty blocks.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summarized: Option<bool>,
}
