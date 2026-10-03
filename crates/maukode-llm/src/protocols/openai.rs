use crate::client::{LLMClient, LLMStream};
use crate::error::LLMError;
use crate::request::LLMRequest;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{stream, StreamExt};
use maukode_protocol::{ContentPart, FinishReason, LLMEvent, Role, TokenUsage, ToolCall};
use serde_json::json;
use std::collections::{BTreeMap, VecDeque};

#[derive(Default)]
struct ToolAccumulator {
    id: String,
    name: String,
    args: String,
}

#[derive(Default)]
struct StreamState {
    tools: BTreeMap<usize, ToolAccumulator>,
}

impl StreamState {
    fn consume(&mut self, val: &serde_json::Value) -> Vec<Result<LLMEvent, LLMError>> {
        let mut events = Vec::new();
        if let Some(choice) = val.get("choices").and_then(|c| c.get(0)) {
            let delta = choice.get("delta");
            if let Some(text) = delta
                .and_then(|d| d.get("content"))
                .and_then(|t| t.as_str())
            {
                if !text.is_empty() {
                    events.push(Ok(LLMEvent::TextDelta { text: text.into() }));
                }
            }
            if let Some(text) = delta
                .and_then(|d| d.get("reasoning_content"))
                .and_then(|t| t.as_str())
            {
                if !text.is_empty() {
                    events.push(Ok(LLMEvent::ReasoningDelta { text: text.into() }));
                }
            }
            if let Some(tool_calls) = delta
                .and_then(|d| d.get("tool_calls"))
                .and_then(|t| t.as_array())
            {
                for tool_call in tool_calls {
                    let index = tool_call.get("index").and_then(|i| i.as_u64()).unwrap_or(0);
                    let tool = self.tools.entry(index as usize).or_default();
                    if let Some(id) = tool_call.get("id").and_then(|i| i.as_str()) {
                        if !id.starts_with(&tool.id) {
                            tool.id.push_str(id);
                        } else {
                            tool.id = id.to_string();
                        }
                    }
                    if let Some(function) = tool_call.get("function") {
                        if let Some(name) = function.get("name").and_then(|n| n.as_str()) {
                            if !name.is_empty() {
                                if tool.name.is_empty() {
                                    tool.name.push_str(name);
                                    events.push(Ok(LLMEvent::ToolCallStart {
                                        id: tool.id.clone(),
                                        name: tool.name.clone(),
                                    }));
                                } else if name.starts_with(&tool.name) {
                                    tool.name = name.to_string();
                                } else {
                                    tool.name.push_str(name);
                                }
                            }
                        }
                        if let Some(args) = function.get("arguments").and_then(|a| a.as_str()) {
                            tool.args.push_str(args);
                            events.push(Ok(LLMEvent::ToolCallDelta {
                                id: tool.id.clone(),
                                delta: args.to_string(),
                            }));
                        }
                    }
                }
            }
            if let Some(reason) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                for (_, tool) in std::mem::take(&mut self.tools) {
                    if tool.name.is_empty() {
                        events.push(Err(LLMError::Stream("Tool call has no name".into())));
                        continue;
                    }
                    match serde_json::from_str(&tool.args) {
                        Ok(arguments) => events.push(Ok(LLMEvent::ToolCallFinish {
                            tool_call: ToolCall {
                                id: tool.id,
                                name: tool.name,
                                arguments,
                            },
                        })),
                        Err(err) => events.push(Err(LLMError::Stream(format!(
                            "Invalid arguments for tool '{}': {err}",
                            tool.name
                        )))),
                    }
                }
                let reason = match reason {
                    "tool_calls" => FinishReason::ToolUse,
                    "length" => FinishReason::MaxTokens,
                    _ => FinishReason::EndTurn,
                };
                events.push(Ok(LLMEvent::Finish { reason }));
            }
        }
        if let Some(usage) = val.get("usage").and_then(|u| u.as_object()) {
            let input_tokens = usage
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let output_tokens = usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if input_tokens > 0 || output_tokens > 0 {
                events.push(Ok(LLMEvent::Usage(TokenUsage {
                    input_tokens,
                    output_tokens,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                })));
            }
        }
        events
    }
}

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
                    ContentPart::ProviderItem { .. } => {}
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

        let stream = stream::unfold(
            (event_source, StreamState::default(), VecDeque::new()),
            |(mut source, mut state, mut pending)| async move {
                loop {
                    if let Some(event) = pending.pop_front() {
                        return Some((event, (source, state, pending)));
                    }
                    let event = source.next().await?;
                    let event = match event {
                        Ok(event) => event,
                        Err(err) => {
                            return Some((
                                Err(LLMError::Stream(err.to_string())),
                                (source, state, pending),
                            ));
                        }
                    };
                    if event.data == "[DONE]" {
                        return None;
                    }
                    let value: serde_json::Value = match serde_json::from_str(&event.data) {
                        Ok(value) => value,
                        Err(err) => {
                            return Some((
                                Err(LLMError::Stream(format!("Invalid SSE JSON: {err}"))),
                                (source, state, pending),
                            ));
                        }
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn fragmented_tool_name_and_arguments_are_assembled() {
        let mut state = StreamState::default();
        let first = state.consume(&json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "id": "call_1", "function": {"name": "gre", "arguments": r#"{"pattern":"#}
        }]}}]}));
        assert!(
            matches!(first.as_slice(), [Ok(LLMEvent::ToolCallStart { name, .. }), Ok(LLMEvent::ToolCallDelta { .. })] if name == "gre")
        );

        let second = state.consume(&json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "function": {"name": "p", "arguments": r#""hello"}"#}
        }]}}]}));
        assert!(matches!(
            second.as_slice(),
            [Ok(LLMEvent::ToolCallDelta { .. })]
        ));

        let done = state.consume(&json!({"choices": [{"finish_reason": "tool_calls"}]}));
        assert!(
            matches!(done.as_slice(), [Ok(LLMEvent::ToolCallFinish { tool_call }), Ok(LLMEvent::Finish { reason: FinishReason::ToolUse })]
            if tool_call.name == "grep" && tool_call.arguments == json!({"pattern": "hello"}))
        );

        let mut cumulative = StreamState::default();
        cumulative.consume(&json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "id": "call_2", "function": {"name": "gre", "arguments": "{}"}
        }]}}]}));
        cumulative.consume(&json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "function": {"name": "grep"}
        }]}}]}));
        let done = cumulative.consume(&json!({"choices": [{"finish_reason": "tool_calls"}]}));
        assert!(
            matches!(&done[0], Ok(LLMEvent::ToolCallFinish { tool_call }) if tool_call.name == "grep")
        );
    }

    #[test]
    fn all_tool_calls_and_mixed_deltas_are_preserved() {
        let mut state = StreamState::default();
        let events = state.consume(&json!({"choices": [{"delta": {
            "content": "Searching", "tool_calls": [
                {"index": 1, "id": "call_2", "function": {"name": "glob", "arguments": "{}"}},
                {"index": 0, "id": "call_1", "function": {"name": "grep", "arguments": "{}"}}
            ]
        }}]}));
        assert_eq!(events.len(), 5);
        assert!(matches!(&events[0], Ok(LLMEvent::TextDelta { text }) if text == "Searching"));

        let done = state.consume(&json!({"choices": [{"finish_reason": "tool_calls"}]}));
        assert!(
            matches!(&done[0], Ok(LLMEvent::ToolCallFinish { tool_call }) if tool_call.name == "grep")
        );
        assert!(
            matches!(&done[1], Ok(LLMEvent::ToolCallFinish { tool_call }) if tool_call.name == "glob")
        );
        assert!(matches!(
            &done[2],
            Ok(LLMEvent::Finish {
                reason: FinishReason::ToolUse
            })
        ));
    }

    #[test]
    fn invalid_tool_arguments_are_not_executed() {
        let mut state = StreamState::default();
        state.consume(&json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "id": "call_1", "function": {"name": "grep", "arguments": "{"}
        }]}}]}));
        let done = state.consume(&json!({"choices": [{"finish_reason": "tool_calls"}]}));
        assert!(matches!(&done[0], Err(LLMError::Stream(_))));
        assert!(!done
            .iter()
            .any(|event| matches!(event, Ok(LLMEvent::ToolCallFinish { .. }))));
    }

    #[tokio::test]
    async fn local_sse_gateway_stream_assembles_tool_name() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let header_end = loop {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                if let Some(offset) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    break offset + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]);
            assert!(headers.starts_with("POST /v1/chat/completions HTTP/1.1"));
            let body_length: usize = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse().ok())
                })
                .unwrap();
            while request.len() - header_end < body_length {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let body: serde_json::Value =
                serde_json::from_slice(&request[header_end..header_end + body_length]).unwrap();
            assert_eq!(body["tools"][0]["function"]["name"], "grep");

            let events = [
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "call_1", "function": {"name": "gre", "arguments": "{"}}]}}]}),
                json!({"choices": [{"delta": {"tool_calls": [{"index": 0, "function": {"name": "p", "arguments": "}"}}]}}]}),
                json!({"choices": [{"finish_reason": "tool_calls"}]}),
            ];
            let payload = events
                .iter()
                .map(|event| format!("data: {event}\n\n"))
                .collect::<String>()
                + "data: [DONE]\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n{payload}"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });

        let client = OpenAIClient::new("test-key").with_base_url(format!("http://{address}"));
        let request = LLMRequest::new("test-model", vec![maukode_protocol::Message::user("find")])
            .with_tools(vec![maukode_tools::ToolDefinition {
                name: "grep".into(),
                description: "Search text".into(),
                parameters: json!({"type": "object"}),
            }]);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.stream(request).await.unwrap().collect::<Vec<_>>(),
        )
        .await;
        server.abort();
        let events = result.expect("[DONE] should end the stream before the server closes");
        assert!(events.iter().any(|event| matches!(event, Ok(LLMEvent::ToolCallFinish { tool_call }) if tool_call.name == "grep")));
    }
}
