use crate::client::{LLMClient, LLMStream};
use crate::error::LLMError;
use crate::request::LLMRequest;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{stream, StreamExt};
use maukode_protocol::{ContentPart, FinishReason, LLMEvent, Role, TokenUsage, ToolCall};
use serde_json::{json, Value};
use std::collections::VecDeque;

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
            http: crate::http_client(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

fn cache_breakpoint() -> Value {
    json!({ "type": "ephemeral" })
}

/// Build the Messages API body. Three cache breakpoints are set, in prefix order: the last tool,
/// the stable part of the system prompt, and the last message. Within a turn every step then
/// reads the whole previous request from the cache; across turns at least the tools and the
/// stable system prompt are reused.
fn build_body(request: &LLMRequest) -> Value {
    let mut messages = Vec::new();
    for msg in &request.messages {
        let role = match msg.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            // System messages are placed in the top-level system parameter.
            Role::System => continue,
        };

        let mut contents = Vec::new();
        for part in &msg.parts {
            match part {
                ContentPart::Text { text } => {
                    contents.push(json!({ "type": "text", "text": text }));
                }
                ContentPart::Reasoning { .. } | ContentPart::ProviderItem { .. } => {}
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

        messages.push(json!({ "role": role, "content": contents }));
    }
    if let Some(block) = messages
        .last_mut()
        .and_then(|m| m["content"].as_array_mut())
        .and_then(|blocks| blocks.last_mut())
    {
        block["cache_control"] = cache_breakpoint();
    }

    let mut tools: Vec<Value> = request
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
    if let Some(last) = tools.last_mut() {
        last["cache_control"] = cache_breakpoint();
    }

    let mut system = Vec::new();
    if let Some(prompt) = request.system_prompt.as_deref().filter(|p| !p.is_empty()) {
        system.push(json!({ "type": "text", "text": prompt, "cache_control": cache_breakpoint() }));
    }
    if let Some(suffix) = &request.system_suffix {
        system.push(json!({ "type": "text", "text": suffix }));
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": request.max_tokens.unwrap_or(4096),
        "stream": true
    });
    if !system.is_empty() {
        body["system"] = json!(system);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(temp) = request.temperature {
        body["temperature"] = json!(temp);
    }
    body
}

#[derive(Default)]
struct StreamState {
    current_tool_id: Option<String>,
    current_tool_name: Option<String>,
    tool_args_buf: String,
    usage: TokenUsage,
    /// `message_stop` or an `error` event was received; nothing valid can follow.
    done: bool,
}

impl StreamState {
    fn consume(&mut self, val: &Value) -> Vec<Result<LLMEvent, LLMError>> {
        match val["type"].as_str().unwrap_or("") {
            "message_start" => {
                if let Some(u) = val["message"]["usage"].as_object() {
                    self.usage.input_tokens =
                        u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                    self.usage.cache_read_tokens =
                        u.get("cache_read_input_tokens").and_then(|v| v.as_u64());
                    self.usage.cache_write_tokens = u
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64());
                }
                Vec::new()
            }
            "content_block_start" => {
                let block = &val["content_block"];
                if block["type"] != "tool_use" {
                    return Vec::new();
                }
                let id = block["id"].as_str().unwrap_or("").to_string();
                let name = block["name"].as_str().unwrap_or("").to_string();
                self.current_tool_id = Some(id.clone());
                self.current_tool_name = Some(name.clone());
                self.tool_args_buf.clear();
                vec![Ok(LLMEvent::ToolCallStart { id, name })]
            }
            "content_block_delta" => {
                let delta = &val["delta"];
                match delta["type"].as_str().unwrap_or("") {
                    "text_delta" => {
                        let text = delta["text"].as_str().unwrap_or("").to_string();
                        vec![Ok(LLMEvent::TextDelta { text })]
                    }
                    "input_json_delta" => {
                        let partial = delta["partial_json"].as_str().unwrap_or("");
                        self.tool_args_buf.push_str(partial);
                        self.current_tool_id
                            .iter()
                            .map(|id| {
                                Ok(LLMEvent::ToolCallDelta {
                                    id: id.clone(),
                                    delta: partial.to_string(),
                                })
                            })
                            .collect()
                    }
                    _ => Vec::new(),
                }
            }
            "content_block_stop" => {
                let (Some(id), Some(name)) =
                    (self.current_tool_id.take(), self.current_tool_name.take())
                else {
                    return Vec::new();
                };
                let args = std::mem::take(&mut self.tool_args_buf);
                // A tool without parameters may stream no input at all.
                let parsed = if args.trim().is_empty() {
                    Ok(Value::Object(Default::default()))
                } else {
                    serde_json::from_str(&args)
                };
                match parsed {
                    Ok(arguments) => vec![Ok(LLMEvent::ToolCallFinish {
                        tool_call: ToolCall {
                            id,
                            name,
                            arguments,
                        },
                    })],
                    Err(err) => vec![Err(LLMError::Stream(format!(
                        "Invalid arguments for tool '{name}': {err}"
                    )))],
                }
            }
            "message_delta" => {
                if let Some(u) = val["usage"].as_object() {
                    self.usage.output_tokens =
                        u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                }
                let reason = match val["delta"]["stop_reason"].as_str() {
                    Some("tool_use") => FinishReason::ToolUse,
                    Some("max_tokens") => FinishReason::MaxTokens,
                    _ => FinishReason::EndTurn,
                };
                vec![
                    Ok(LLMEvent::Usage(self.usage.clone())),
                    Ok(LLMEvent::Finish { reason }),
                ]
            }
            "message_stop" => {
                self.done = true;
                Vec::new()
            }
            "error" => {
                self.done = true;
                let error = &val["error"];
                let kind = error["type"].as_str().unwrap_or("error");
                let message = error["message"].as_str().unwrap_or("");
                // `overloaded_error` and `api_error` contain words that `is_transient` retries.
                vec![Err(LLMError::Stream(format!("{kind}: {message}")))]
            }
            _ => Vec::new(),
        }
    }
}

#[async_trait]
impl LLMClient for AnthropicClient {
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let body = build_body(&request);
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
        let stream = stream::unfold(
            (event_source, StreamState::default(), VecDeque::new()),
            |(mut source, mut state, mut pending)| async move {
                loop {
                    if let Some(event) = pending.pop_front() {
                        return Some((event, (source, state, pending)));
                    }
                    if state.done {
                        return None;
                    }
                    let event = match source.next().await {
                        None => return None,
                        Some(Ok(event)) => event,
                        Some(Err(err)) => {
                            state.done = true;
                            return Some((
                                Err(LLMError::Stream(err.to_string())),
                                (source, state, pending),
                            ));
                        }
                    };
                    let value: Value = match serde_json::from_str(&event.data) {
                        Ok(value) => value,
                        // Pings and other non-JSON payloads carry nothing.
                        Err(_) => continue,
                    };
                    pending.extend(state.consume(&value));
                }
            },
        );

        Ok(Box::pin(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maukode_protocol::Message;

    fn tool(name: &str) -> maukode_tools::ToolDefinition {
        maukode_tools::ToolDefinition {
            name: name.into(),
            description: "A tool".into(),
            parameters: json!({"type": "object"}),
            source: maukode_tools::ToolSource::Native,
        }
    }

    #[test]
    fn request_marks_tools_stable_system_prompt_and_last_message_for_caching() {
        let request = LLMRequest::new(
            "model",
            vec![Message::user("first"), Message::user("second")],
        )
        .with_system_prompt("stable")
        .with_system_suffix("today")
        .with_tools(vec![tool("a"), tool("b")]);
        let body = build_body(&request);

        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["system"][0]["text"], "stable");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["system"][1]["text"], "today");
        assert!(body["system"][1].get("cache_control").is_none());
        assert!(body["messages"][0]["content"][0].get("cache_control").is_none());
        assert_eq!(
            body["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn stream_reports_usage_and_tool_calls() {
        let mut state = StreamState::default();
        state.consume(&json!({"type": "message_start", "message": {"usage": {
            "input_tokens": 10, "cache_read_input_tokens": 90
        }}}));
        state.consume(&json!({"type": "content_block_start", "content_block": {
            "type": "tool_use", "id": "t1", "name": "grep"
        }}));
        state.consume(&json!({"type": "content_block_delta", "delta": {
            "type": "input_json_delta", "partial_json": "{\"query\":\"x\"}"
        }}));
        let finished = state.consume(&json!({"type": "content_block_stop"}));
        assert!(matches!(
            finished.as_slice(),
            [Ok(LLMEvent::ToolCallFinish { tool_call })]
            if tool_call.name == "grep" && tool_call.arguments == json!({"query": "x"})
        ));

        let end = state.consume(&json!({"type": "message_delta",
            "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 5}}));
        assert!(matches!(
            end.as_slice(),
            [Ok(LLMEvent::Usage(usage)), Ok(LLMEvent::Finish { reason: FinishReason::ToolUse })]
            if usage.input_tokens == 10
                && usage.output_tokens == 5
                && usage.cache_read_tokens == Some(90)
        ));
    }

    #[test]
    fn tool_without_input_gets_empty_arguments() {
        let mut state = StreamState::default();
        state.consume(&json!({"type": "content_block_start", "content_block": {
            "type": "tool_use", "id": "t1", "name": "list"
        }}));
        let finished = state.consume(&json!({"type": "content_block_stop"}));
        assert!(matches!(
            finished.as_slice(),
            [Ok(LLMEvent::ToolCallFinish { tool_call })] if tool_call.arguments == json!({})
        ));
    }

    #[test]
    fn malformed_tool_arguments_are_an_error_not_an_empty_call() {
        let mut state = StreamState::default();
        state.consume(&json!({"type": "content_block_start", "content_block": {
            "type": "tool_use", "id": "t1", "name": "bash"
        }}));
        state.consume(&json!({"type": "content_block_delta", "delta": {
            "type": "input_json_delta", "partial_json": "{\"command\": \"rm"
        }}));
        let finished = state.consume(&json!({"type": "content_block_stop"}));
        assert!(matches!(finished.as_slice(), [Err(LLMError::Stream(_))]));
    }

    #[test]
    fn mid_stream_error_event_fails_the_stream_and_overload_is_retried() {
        let mut state = StreamState::default();
        let events = state.consume(&json!({"type": "error", "error": {
            "type": "overloaded_error", "message": "Overloaded"
        }}));
        assert!(state.done);
        let [Err(err)] = events.as_slice() else {
            panic!("expected one error, got {events:?}");
        };
        assert!(err.to_string().contains("overloaded_error"));
        assert!(err.is_transient());
    }
}
