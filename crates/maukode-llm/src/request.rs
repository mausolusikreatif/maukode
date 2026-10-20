use maukode_protocol::Message;
use maukode_tools::ToolDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    /// Context that changes between turns (date, recalled memories). Adapters send it after the
    /// conversation, as [`Self::turn_context_note`] appended to the last user message, so that
    /// the tools, the system prompt and the whole history before it stay a cacheable prefix
    /// across turns. When there is no user message to append it to, it follows the system prompt.
    #[serde(default)]
    pub system_suffix: Option<String>,
    pub tools: Vec<ToolDefinition>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

impl LLMRequest {
    pub fn new(model: impl Into<String>, messages: Vec<Message>) -> Self {
        Self {
            model: model.into(),
            messages,
            system_prompt: None,
            system_suffix: None,
            tools: Vec::new(),
            temperature: None,
            max_tokens: Some(4096),
        }
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    pub fn with_system_suffix(mut self, suffix: impl Into<String>) -> Self {
        let suffix = suffix.into();
        self.system_suffix = (!suffix.trim().is_empty()).then_some(suffix);
        self
    }

    /// `system_suffix` as the note adapters append to the last user message.
    pub fn turn_context_note(&self) -> Option<String> {
        self.system_suffix
            .as_deref()
            .map(|context| format!("<maukode-context>\n{}\n</maukode-context>", context.trim()))
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
