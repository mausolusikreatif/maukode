use maukode_protocol::Message;
use maukode_tools::ToolDefinition;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub system_prompt: Option<String>,
    /// The part of the system prompt that changes between turns (date, recalled memories). It
    /// is sent after `system_prompt` so that the stable part stays a cacheable prefix.
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

    /// The whole system prompt for providers that take it as one string.
    pub fn full_system_prompt(&self) -> Option<String> {
        match (&self.system_prompt, &self.system_suffix) {
            (Some(prompt), Some(suffix)) => Some(format!("{prompt}\n{suffix}")),
            (Some(text), None) | (None, Some(text)) => Some(text.clone()),
            (None, None) => None,
        }
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
