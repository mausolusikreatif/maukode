use crate::client::{LLMClient, LLMStream};
use crate::error::LLMError;
use crate::request::{LLMRequest, ThinkingMode};
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

/// Beta that lets a request choose what happens to a thinking block whose conversation changed.
const THINKING_BINDING_BETA: &str = "thinking-binding-controls-2026-08-01";

/// `(major, minor)` of a Claude model id such as `claude-opus-4-6`, `claude-sonnet-5` or
/// `anthropic/claude-haiku-5-5`. `None` for other forms, including the older
/// `claude-3-5-sonnet-…` ids.
fn claude_version(model: &str) -> Option<(u32, u32)> {
    let name = model.rsplit('/').next()?.strip_prefix("claude-")?;
    let mut words = name.split('-');
    let family = words.next()?;
    if family.chars().next()?.is_ascii_digit() {
        return None;
    }
    let major = words.next()?.parse().ok()?;
    // A date suffix (`-20250514`) is not a minor version.
    let minor = words
        .next()
        .filter(|w| w.len() <= 2)
        .and_then(|w| w.parse().ok())
        .unwrap_or(0);
    Some((major, minor))
}

/// The `thinking` request parameter, if any. Claude 4.6 and later think adaptively; Claude 5 and
/// later always think, so `Off` cannot turn it off there. Blocks from those models are bound to
/// the conversation that produced them: when an earlier message changed (pruning, compaction),
/// the API is asked to drop the stale blocks instead of rejecting the request.
fn thinking_param(request: &LLMRequest) -> Option<Value> {
    let version = claude_version(&request.model);
    let adaptive_capable = version.is_some_and(|v| v >= (4, 6));
    let mode = match request.thinking.mode {
        ThinkingMode::Off if version.is_some_and(|v| v.0 >= 5) => ThinkingMode::Adaptive,
        ThinkingMode::Auto if adaptive_capable => ThinkingMode::Adaptive,
        ThinkingMode::Auto => ThinkingMode::Off,
        mode => mode,
    };
    let mut thinking = match mode {
        ThinkingMode::Adaptive => json!({ "type": "adaptive" }),
        ThinkingMode::Budget => json!({
            "type": "enabled",
            "budget_tokens": thinking_budget(request),
        }),
        ThinkingMode::Auto | ThinkingMode::Off => return None,
    };
    // `display` arrived with Claude 4.7, whose default hides the thinking text.
    if version.is_some_and(|v| v >= (4, 7)) {
        let summarized = request.thinking.summarized.unwrap_or(true);
        thinking["display"] = json!(if summarized { "summarized" } else { "omitted" });
    }
    if adaptive_capable {
        thinking["block_binding"] = json!({ "prefix_mismatch_behavior": "drop_block" });
    }
    Some(thinking)
}

fn thinking_budget(request: &LLMRequest) -> u32 {
    request.thinking.budget_tokens.unwrap_or(4096).max(1024)
}

/// Whether a stored provider item is an Anthropic thinking block (other providers' items, such
/// as ChatGPT reasoning, are not sent here).
fn is_thinking_block(value: &Value) -> bool {
    matches!(
        value["type"].as_str(),
        Some("thinking" | "redacted_thinking")
    )
}

/// Build the Messages API body. Three cache breakpoints are set, in prefix order: the last tool,
/// the system prompt, and the last message. The history is append-only (per-turn context is
/// stored in its turn's user message), so every step and every later turn reads the earlier
/// conversation from the cache, and replayed thinking blocks stay valid.
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
                ContentPart::Context { text } => {
                    contents.push(json!({ "type": "text", "text": crate::context_text(text) }));
                }
                ContentPart::Image { media_type, data } => {
                    contents.push(json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": media_type, "data": data },
                    }));
                }
                // Thinking blocks go back unchanged, in their original position.
                ContentPart::ProviderItem { value }
                    if msg.role == Role::Assistant && is_thinking_block(value) =>
                {
                    contents.push(value.clone());
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
    // Thinking blocks cannot carry a breakpoint.
    if let Some(block) = messages
        .last_mut()
        .and_then(|m| m["content"].as_array_mut())
        .and_then(|blocks| blocks.iter_mut().rev().find(|b| !is_thinking_block(b)))
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

    let thinking = thinking_param(request);
    let mut max_tokens = request.max_tokens.unwrap_or(4096);
    if thinking.as_ref().is_some_and(|t| t["type"] == "enabled") {
        // The budget is part of `max_tokens` and must leave room for the answer.
        max_tokens = max_tokens.max(thinking_budget(request) + 1024);
    }

    let mut body = json!({
        "model": request.model,
        "messages": messages,
        "max_tokens": max_tokens,
        "stream": true
    });
    if let Some(prompt) = request.system_prompt.as_deref().filter(|p| !p.is_empty()) {
        body["system"] = json!([
            { "type": "text", "text": prompt, "cache_control": cache_breakpoint() }
        ]);
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if let Some(effort) = &request.thinking.effort {
        body["output_config"] = json!({ "effort": effort });
    }
    match thinking {
        Some(thinking) => body["thinking"] = thinking,
        // Thinking does not accept a changed temperature.
        None => {
            if let Some(temp) = request.temperature {
                body["temperature"] = json!(temp);
            }
        }
    }
    body
}

/// A thinking block being streamed: its text and signature, or a redacted block's data.
enum ThinkingBlock {
    Visible { text: String, signature: String },
    Redacted { data: Value },
}

#[derive(Default)]
struct StreamState {
    thinking: Option<ThinkingBlock>,
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
                    self.usage.cache_read_tokens =
                        u.get("cache_read_input_tokens").and_then(|v| v.as_u64());
                    self.usage.cache_write_tokens = u
                        .get("cache_creation_input_tokens")
                        .and_then(|v| v.as_u64());
                    // Anthropic counts cached tokens separately; `TokenUsage::input_tokens` is
                    // the whole prompt, as with the other providers.
                    self.usage.input_tokens =
                        u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                            + self.usage.cache_read_tokens.unwrap_or(0)
                            + self.usage.cache_write_tokens.unwrap_or(0);
                }
                Vec::new()
            }
            "content_block_start" => {
                let block = &val["content_block"];
                match block["type"].as_str() {
                    Some("thinking") => {
                        let text = block["thinking"].as_str().unwrap_or_default().to_string();
                        let signature = block["signature"].as_str().unwrap_or_default();
                        self.thinking = Some(ThinkingBlock::Visible {
                            text: text.clone(),
                            signature: signature.to_string(),
                        });
                        return if text.is_empty() {
                            Vec::new()
                        } else {
                            vec![Ok(LLMEvent::ReasoningDelta { text })]
                        };
                    }
                    Some("redacted_thinking") => {
                        self.thinking = Some(ThinkingBlock::Redacted {
                            data: block["data"].clone(),
                        });
                        return Vec::new();
                    }
                    Some("tool_use") => {}
                    _ => return Vec::new(),
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
                    "thinking_delta" => {
                        let more = delta["thinking"].as_str().unwrap_or_default();
                        if let Some(ThinkingBlock::Visible { text, .. }) = &mut self.thinking {
                            text.push_str(more);
                        }
                        if more.is_empty() {
                            Vec::new()
                        } else {
                            vec![Ok(LLMEvent::ReasoningDelta {
                                text: more.to_string(),
                            })]
                        }
                    }
                    "signature_delta" => {
                        let more = delta["signature"].as_str().unwrap_or_default();
                        if let Some(ThinkingBlock::Visible { signature, .. }) = &mut self.thinking {
                            signature.push_str(more);
                        }
                        Vec::new()
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
                // A finished thinking block is kept verbatim: it must go back unchanged.
                if let Some(block) = self.thinking.take() {
                    let item = match block {
                        ThinkingBlock::Visible { text, signature } => json!({
                            "type": "thinking",
                            "thinking": text,
                            "signature": signature,
                        }),
                        ThinkingBlock::Redacted { data } => {
                            json!({ "type": "redacted_thinking", "data": data })
                        }
                    };
                    return vec![Ok(LLMEvent::ProviderItem { item })];
                }
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
        let mut builder = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");
        if body["thinking"].get("block_binding").is_some() {
            builder = builder.header("anthropic-beta", THINKING_BINDING_BETA);
        }
        let resp = builder.json(&body).send().await?;

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
    use crate::request::ThinkingSettings;
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
        let mut first = Message::user("first");
        first.parts.push(ContentPart::Context {
            text: "today".into(),
        });
        let request = LLMRequest::new("model", vec![first, Message::user("second")])
            .with_system_prompt("stable")
            .with_tools(vec![tool("a"), tool("b")]);
        let body = build_body(&request);

        assert!(body["tools"][0].get("cache_control").is_none());
        assert_eq!(body["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(body["system"][0]["text"], "stable");
        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(body["system"].get(1).is_none());
        // The turn context stays in the message it was added to.
        assert_eq!(
            body["messages"][0]["content"][1]["text"],
            "<maukode-context>\ntoday\n</maukode-context>"
        );
        assert!(body["messages"][0]["content"][1]
            .get("cache_control")
            .is_none());
        assert_eq!(
            body["messages"][1]["content"][0]["cache_control"]["type"],
            "ephemeral"
        );
    }

    #[test]
    fn images_and_thinking_blocks_are_sent_in_place() {
        let thinking = json!({"type": "thinking", "thinking": "hmm", "signature": "sig"});
        let request = LLMRequest::new(
            "claude-opus-5-5",
            vec![
                Message::user_with_images(
                    "what is this",
                    vec![ContentPart::Image {
                        media_type: "image/png".into(),
                        data: "aGk=".into(),
                    }],
                ),
                Message::assistant(vec![
                    ContentPart::ProviderItem {
                        value: thinking.clone(),
                    },
                    ContentPart::Reasoning { text: "hmm".into() },
                    ContentPart::Text { text: "A".into() },
                    ContentPart::ProviderItem {
                        value: json!({"type": "reasoning", "encrypted_content": "x"}),
                    },
                ]),
            ],
        );
        let body = build_body(&request);

        let image = &body["messages"][0]["content"][0];
        assert_eq!(image["type"], "image");
        assert_eq!(image["source"]["media_type"], "image/png");
        assert_eq!(image["source"]["data"], "aGk=");
        let assistant = body["messages"][1]["content"].as_array().unwrap();
        // The thinking block comes back unchanged and first; other providers' items are dropped.
        assert_eq!(assistant.len(), 2);
        assert_eq!(assistant[0], thinking);
        assert_eq!(assistant[1]["text"], "A");
        assert_eq!(assistant[1]["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn thinking_follows_the_model_generation() {
        let thinking = |model: &str, settings: ThinkingSettings| {
            let mut request = LLMRequest::new(model, vec![Message::user("hi")]);
            request.thinking = settings;
            request.max_tokens = Some(2000);
            build_body(&request)
        };
        let auto = ThinkingSettings::default();

        let body = thinking("claude-opus-5-5", auto.clone());
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert_eq!(body["thinking"]["display"], "summarized");
        assert_eq!(
            body["thinking"]["block_binding"]["prefix_mismatch_behavior"],
            "drop_block"
        );
        // Claude 5 models always think: `off` keeps adaptive thinking.
        let off = ThinkingSettings {
            mode: ThinkingMode::Off,
            ..ThinkingSettings::default()
        };
        assert_eq!(
            thinking("claude-sonnet-5-5", off.clone())["thinking"]["type"],
            "adaptive"
        );
        assert!(thinking("claude-opus-4-8", off).get("thinking").is_none());
        // 4.6 is adaptive but predates `display`.
        let body = thinking("claude-sonnet-4-6", auto.clone());
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body["thinking"].get("display").is_none());
        // Older models think only with a budget, which must fit in `max_tokens`.
        assert!(thinking("claude-haiku-4-5", auto).get("thinking").is_none());
        let budget = ThinkingSettings {
            mode: ThinkingMode::Budget,
            budget_tokens: Some(3000),
            effort: Some("high".into()),
            ..ThinkingSettings::default()
        };
        let body = thinking("claude-haiku-4-5-20251001", budget);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["thinking"]["budget_tokens"], 3000);
        assert!(body["thinking"].get("block_binding").is_none());
        assert_eq!(body["max_tokens"], 4024);
        assert_eq!(body["output_config"]["effort"], "high");
    }

    #[test]
    fn model_versions_are_parsed_from_current_and_gateway_ids() {
        assert_eq!(claude_version("claude-opus-4-6"), Some((4, 6)));
        assert_eq!(claude_version("claude-sonnet-5"), Some((5, 0)));
        assert_eq!(claude_version("claude-sonnet-4-20250514"), Some((4, 0)));
        assert_eq!(claude_version("anthropic/claude-haiku-5-5"), Some((5, 5)));
        assert_eq!(claude_version("claude-3-7-sonnet-latest"), None);
        assert_eq!(claude_version("gpt-5"), None);
    }

    #[test]
    fn thinking_blocks_are_streamed_as_reasoning_and_kept_verbatim() {
        let mut state = StreamState::default();
        state.consume(&json!({"type": "content_block_start", "content_block": {
            "type": "thinking", "thinking": "", "signature": ""
        }}));
        let delta = state.consume(&json!({"type": "content_block_delta", "delta": {
            "type": "thinking_delta", "thinking": "Let me see"
        }}));
        assert!(matches!(delta.as_slice(),
            [Ok(LLMEvent::ReasoningDelta { text })] if text == "Let me see"));
        state.consume(&json!({"type": "content_block_delta", "delta": {
            "type": "signature_delta", "signature": "abc"
        }}));
        let done = state.consume(&json!({"type": "content_block_stop"}));
        assert!(
            matches!(done.as_slice(), [Ok(LLMEvent::ProviderItem { item })]
            if *item == json!({"type": "thinking", "thinking": "Let me see", "signature": "abc"}))
        );

        state.consume(&json!({"type": "content_block_start", "content_block": {
            "type": "redacted_thinking", "data": "opaque"
        }}));
        let done = state.consume(&json!({"type": "content_block_stop"}));
        assert!(
            matches!(done.as_slice(), [Ok(LLMEvent::ProviderItem { item })]
            if *item == json!({"type": "redacted_thinking", "data": "opaque"}))
        );
    }

    #[test]
    fn stream_reports_usage_and_tool_calls() {
        let mut state = StreamState::default();
        state.consume(&json!({"type": "message_start", "message": {"usage": {
            "input_tokens": 10, "cache_read_input_tokens": 90, "cache_creation_input_tokens": 20
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
            if usage.input_tokens == 120
                && usage.output_tokens == 5
                && usage.cache_read_tokens == Some(90)
                && usage.cache_write_tokens == Some(20)
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
