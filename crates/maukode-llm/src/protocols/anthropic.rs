use crate::client::{LLMClient, LLMStream};
use crate::error::LLMError;
use crate::request::LLMRequest;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{StreamExt, TryStreamExt};
use maukode_protocol::{ContentPart, FinishReason, LLMEvent, Role, TokenUsage, ToolCall};
use serde_json::json;
use std::sync::Arc;

pub struct AnthropicClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl AnthropicClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com".to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

#[async_trait]
impl LLMClient for AnthropicClient {
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        // Convert messages to Anthropic format
        let mut anthropic_messages = Vec::new();
        for msg in &request.messages {
            if msg.role == Role::System {
                continue; // System messages are placed in top-level system parameter
            }

            let role_str = match msg.role {
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::System => unreachable!(),
            };

            let mut contents = Vec::new();
            for part in &msg.parts {
                match part {
                    ContentPart::Text { text } => {
                        contents.push(json!({
                            "type": "text",
                            "text": text
                        }));
                    }
                    ContentPart::Reasoning { .. } => {}
                    ContentPart::ProviderItem { .. } => {}
                    ContentPart::ToolCall(call) => {
                        contents.push(json!({
                            "type": "tool_use",
                            "id": call.id,
                            "name": call.name,
                            "input": call.arguments
                        }));
                    }
                    ContentPart::ToolResult(res) => {
                        contents.push(json!({
                            "type": "tool_result",
                            "tool_use_id": res.tool_call_id,
                            "content": res.content,
                            "is_error": res.is_error
                        }));
                    }
                }
            }

            anthropic_messages.push(json!({
                "role": role_str,
                "content": contents
            }));
        }

        // Tools
        let tools: Vec<serde_json::Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters
                })
            })
            .collect();

        let mut body = json!({
            "model": request.model,
            "messages": anthropic_messages,
            "max_tokens": request.max_tokens.unwrap_or(4096),
            "stream": true
        });

        if let Some(sys) = request.system_prompt {
            body["system"] = json!(sys);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }

        let url = format!("{}/v1/messages", self.base_url);
        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(LLMError::Api { status, body: text });
        }

        let event_source = resp.bytes_stream().eventsource();

        // State machine for assembling tools during streaming
        struct StreamState {
            current_tool_id: Option<String>,
            current_tool_name: Option<String>,
            tool_args_buf: String,
            usage: TokenUsage,
        }

        let state = Arc::new(tokio::sync::Mutex::new(StreamState {
            current_tool_id: None,
            current_tool_name: None,
            tool_args_buf: String::new(),
            usage: TokenUsage::default(),
        }));

        let stream = event_source
            .map_err(|e| LLMError::Stream(e.to_string()))
            .filter_map(move |event_res| {
                let state = Arc::clone(&state);
                async move {
                    let event = match event_res {
                        Ok(ev) => ev,
                        Err(e) => return Some(Err(e)),
                    };

                    if event.data == "[DONE]" {
                        return None;
                    }

                    let val: serde_json::Value = match serde_json::from_str(&event.data) {
                        Ok(v) => v,
                        Err(_) => return None,
                    };

                    let event_type = val["type"].as_str().unwrap_or("");
                    let mut lock = state.lock().await;

                    match event_type {
                        "message_start" => {
                            if let Some(u) = val["message"]["usage"].as_object() {
                                lock.usage.input_tokens =
                                    u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                                lock.usage.cache_read_tokens =
                                    u.get("cache_read_input_tokens").and_then(|v| v.as_u64());
                                lock.usage.cache_write_tokens = u
                                    .get("cache_creation_input_tokens")
                                    .and_then(|v| v.as_u64());
                            }
                            None
                        }
                        "content_block_start" => {
                            let block = &val["content_block"];
                            if block["type"] == "tool_use" {
                                let id = block["id"].as_str().unwrap_or("").to_string();
                                let name = block["name"].as_str().unwrap_or("").to_string();
                                lock.current_tool_id = Some(id.clone());
                                lock.current_tool_name = Some(name.clone());
                                lock.tool_args_buf.clear();
                                Some(Ok(LLMEvent::ToolCallStart { id, name }))
                            } else {
                                None
                            }
                        }
                        "content_block_delta" => {
                            let delta = &val["delta"];
                            let delta_type = delta["type"].as_str().unwrap_or("");
                            if delta_type == "text_delta" {
                                let text = delta["text"].as_str().unwrap_or("").to_string();
                                Some(Ok(LLMEvent::TextDelta { text }))
                            } else if delta_type == "input_json_delta" {
                                let partial = delta["partial_json"].as_str().unwrap_or("");
                                lock.tool_args_buf.push_str(partial);
                                lock.current_tool_id.as_ref().map(|id| {
                                    Ok(LLMEvent::ToolCallDelta {
                                        id: id.clone(),
                                        delta: partial.to_string(),
                                    })
                                })
                            } else {
                                None
                            }
                        }
                        "content_block_stop" => {
                            if let (Some(id), Some(name)) =
                                (lock.current_tool_id.take(), lock.current_tool_name.take())
                            {
                                let args: serde_json::Value =
                                    serde_json::from_str(&lock.tool_args_buf)
                                        .unwrap_or(serde_json::Value::Object(Default::default()));
                                lock.tool_args_buf.clear();
                                Some(Ok(LLMEvent::ToolCallFinish {
                                    tool_call: ToolCall {
                                        id,
                                        name,
                                        arguments: args,
                                    },
                                }))
                            } else {
                                None
                            }
                        }
                        "message_delta" => {
                            if let Some(u) = val["usage"].as_object() {
                                lock.usage.output_tokens =
                                    u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                            }
                            let finish_reason = match val["delta"]["stop_reason"].as_str() {
                                Some("end_turn") | Some("stop_sequence") => FinishReason::EndTurn,
                                Some("tool_use") => FinishReason::ToolUse,
                                Some("max_tokens") => FinishReason::MaxTokens,
                                _ => FinishReason::EndTurn,
                            };
                            Some(Ok(LLMEvent::Finish {
                                reason: finish_reason,
                            }))
                        }
                        _ => None,
                    }
                }
            });

        Ok(Box::pin(stream))
    }
}
