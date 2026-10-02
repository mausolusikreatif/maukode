use crate::client::{LLMClient, LLMStream};
use crate::error::LLMError;
use crate::request::LLMRequest;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{StreamExt, TryStreamExt};
use maukode_protocol::{ContentPart, FinishReason, LLMEvent, Role, TokenUsage, ToolCall};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;

pub struct OpenAIClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl OpenAIClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com".to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let mut messages = Vec::new();

        if let Some(ref sys) = request.system_prompt {
            messages.push(json!({
                "role": "system",
                "content": sys
            }));
        }

        for msg in &request.messages {
            let role = match msg.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            };

            let mut text = String::new();
            let mut tool_calls = Vec::new();

            for part in &msg.parts {
                match part {
                    ContentPart::Text { text: t } => text.push_str(t),
                    ContentPart::Reasoning { .. } => {}
                    ContentPart::ToolCall(tc) => {
                        tool_calls.push(json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments.to_string()
                            }
                        }));
                    }
                    ContentPart::ToolResult(tr) => {
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": tr.tool_call_id,
                            "content": tr.content
                        }));
                    }
                }
            }

            if !text.is_empty() || !tool_calls.is_empty() {
                let mut obj = json!({
                    "role": role,
                    "content": if text.is_empty() { serde_json::Value::Null } else { json!(text) }
                });
                if !tool_calls.is_empty() {
                    obj["tool_calls"] = json!(tool_calls);
                }
                messages.push(obj);
            }
        }

        let tools: Vec<serde_json::Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters
                    }
                })
            })
            .collect();

        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true }
        });

        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }
        if let Some(temp) = request.temperature {
            body["temperature"] = json!(temp);
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }

        let base = self.base_url.trim_end_matches('/');
        let url = if base.ends_with("/chat/completions") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        };
        let mut req_builder = self
            .http
            .post(&url)
            .header("content-type", "application/json")
            .json(&body);

        if !self.api_key.is_empty() {
            req_builder = req_builder.header("authorization", format!("Bearer {}", self.api_key));
        }

        let resp = req_builder.send().await?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(LLMError::Api { status, body: text });
        }

        let event_source = resp.bytes_stream().eventsource();

        struct ToolAccumulator {
            id: String,
            name: String,
            args: String,
        }

        struct StreamState {
            tools: HashMap<usize, ToolAccumulator>,
        }

        let state = Arc::new(tokio::sync::Mutex::new(StreamState {
            tools: HashMap::new(),
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

                    let choice = val.get("choices").and_then(|c| c.get(0));

                    if let Some(choice) = choice {
                        let delta = choice.get("delta");

                        // Text content delta
                        if let Some(text) = delta
                            .and_then(|d| d.get("content"))
                            .and_then(|t| t.as_str())
                        {
                            if !text.is_empty() {
                                return Some(Ok(LLMEvent::TextDelta {
                                    text: text.to_string(),
                                }));
                            }
                        }

                        // Reasoning delta (DeepSeek / OpenAI reasoning)
                        if let Some(reasoning) = delta
                            .and_then(|d| d.get("reasoning_content"))
                            .and_then(|t| t.as_str())
                        {
                            if !reasoning.is_empty() {
                                return Some(Ok(LLMEvent::ReasoningDelta {
                                    text: reasoning.to_string(),
                                }));
                            }
                        }

                        // Tool calls
                        if let Some(tool_calls) = delta
                            .and_then(|d| d.get("tool_calls"))
                            .and_then(|tc| tc.as_array())
                        {
                            let mut lock = state.lock().await;
                            for tc in tool_calls {
                                let index =
                                    tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                                let entry =
                                    lock.tools.entry(index).or_insert_with(|| ToolAccumulator {
                                        id: String::new(),
                                        name: String::new(),
                                        args: String::new(),
                                    });

                                if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                                    entry.id = id.to_string();
                                }
                                if let Some(func) = tc.get("function") {
                                    if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                                        if !name.is_empty() && entry.name.is_empty() {
                                            entry.name = name.to_string();
                                            return Some(Ok(LLMEvent::ToolCallStart {
                                                id: entry.id.clone(),
                                                name: entry.name.clone(),
                                            }));
                                        }
                                    }
                                    if let Some(args_chunk) =
                                        func.get("arguments").and_then(|a| a.as_str())
                                    {
                                        entry.args.push_str(args_chunk);
                                        return Some(Ok(LLMEvent::ToolCallDelta {
                                            id: entry.id.clone(),
                                            delta: args_chunk.to_string(),
                                        }));
                                    }
                                }
                            }
                        }

                        // Finish reason
                        if let Some(finish_reason) =
                            choice.get("finish_reason").and_then(|f| f.as_str())
                        {
                            let mut lock = state.lock().await;
                            // If tools were accumulated, emit finish for them
                            if !lock.tools.is_empty() {
                                let mut tool_results = Vec::new();
                                for (_, tool) in lock.tools.drain() {
                                    if tool.name.is_empty() {
                                        continue;
                                    }
                                    let parsed_args: serde_json::Value =
                                        serde_json::from_str(&tool.args).unwrap_or_default();
                                    tool_results.push(LLMEvent::ToolCallFinish {
                                        tool_call: ToolCall {
                                            id: tool.id,
                                            name: tool.name,
                                            arguments: parsed_args,
                                        },
                                    });
                                }
                                if let Some(first) = tool_results.into_iter().next() {
                                    return Some(Ok(first));
                                }
                            }

                            let reason = match finish_reason {
                                "tool_calls" => FinishReason::ToolUse,
                                "stop" => FinishReason::EndTurn,
                                "length" => FinishReason::MaxTokens,
                                _ => FinishReason::EndTurn,
                            };
                            return Some(Ok(LLMEvent::Finish { reason }));
                        }
                    }

                    // Check for usage in stream
                    if let Some(usage) = val.get("usage").and_then(|u| u.as_object()) {
                        let prompt_tokens = usage
                            .get("prompt_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let completion_tokens = usage
                            .get("completion_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        if prompt_tokens > 0 || completion_tokens > 0 {
                            return Some(Ok(LLMEvent::Usage(TokenUsage {
                                input_tokens: prompt_tokens,
                                output_tokens: completion_tokens,
                                cache_read_tokens: None,
                                cache_write_tokens: None,
                            })));
                        }
                    }

                    None
                }
            });

        Ok(Box::pin(stream))
    }
}
