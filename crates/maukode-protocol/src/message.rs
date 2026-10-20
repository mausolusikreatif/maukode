use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    Text {
        text: String,
    },
    Reasoning {
        text: String,
    },
    /// Provider-owned response item retained verbatim for stateless continuation.
    ProviderItem {
        value: serde_json::Value,
    },
    ToolCall(ToolCall),
    ToolResult(ToolResult),
    /// Context MauKode adds to a user message (current date, recalled memories). The model sees
    /// it; the UIs do not show it. It stays in place on later requests, so the history remains
    /// append-only.
    Context {
        text: String,
    },
    /// An image the user attached, base64-encoded (no `data:` prefix).
    Image {
        media_type: String,
        data: String,
    },
}

/// Image types every supported provider accepts.
pub const IMAGE_MEDIA_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// Largest image accepted, in bytes before base64 encoding (the Anthropic API's limit).
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Message {
    pub role: Role,
    pub parts: Vec<ContentPart>,
}

impl Message {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            parts: vec![ContentPart::Text { text: text.into() }],
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            parts: vec![ContentPart::Text { text: text.into() }],
        }
    }

    /// A user message from text and attached images (images first, as providers recommend).
    pub fn user_with_images(text: impl Into<String>, images: Vec<ContentPart>) -> Self {
        let mut parts = images;
        parts.push(ContentPart::Text { text: text.into() });
        Self {
            role: Role::User,
            parts,
        }
    }

    pub fn assistant(parts: Vec<ContentPart>) -> Self {
        Self {
            role: Role::Assistant,
            parts,
        }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            parts: vec![ContentPart::Text { text: text.into() }],
        }
    }

    pub fn text_content(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            if let ContentPart::Text { text } = part {
                out.push_str(text);
            }
        }
        out
    }
}
