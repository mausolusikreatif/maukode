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
    /// A chunk with an OpenAI `finish_reason` has already been received.
    ///
    /// Some compatible gateways close the HTTP body immediately after this chunk instead of
    /// sending the optional `[DONE]` SSE sentinel. Their HTTP stacks can surface that close as an
    /// EOF error, even though the model response is complete. We still keep reading after the
    /// finish chunk because a usage-only chunk may follow it.
    terminal_received: bool,
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
                self.terminal_received = true;
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

/// Gateways sometimes deliver a terminal API error as a 200 SSE payload
/// (`data: {"error": {...}}`). Surface it as a real error instead of
/// silently stalling until the connection closes with a bare EOF.
fn gateway_error_message(value: &serde_json::Value) -> Option<String> {
    let err = value.get("error")?;
    if let Some(msg) = err.as_str() {
        if msg.is_empty() {
            return None;
        }
        return Some(format!("Gateway error: {msg}"));
    }
    let message = err.get("message").and_then(|m| m.as_str()).unwrap_or("");
    if message.is_empty() {
        return None;
    }
    match err.get("code").and_then(|c| c.as_str()).unwrap_or("") {
        "" => Some(format!("Gateway error: {message}")),
        code => Some(format!("Gateway error ({code}): {message}")),
    }
}

fn is_stream_terminator(data: &str) -> bool {
    data.trim() == "[DONE]"
}

impl OpenAIClient {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            base_url: "https://api.openai.com".to_string(),
            http: crate::http_client(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    fn chat_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    async fn post_once(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<reqwest::Response, LLMError> {
        let mut req_builder = self
            .http
            .post(url)
            .header("content-type", "application/json")
            .json(body);

        if !self.api_key.is_empty() {
            req_builder = req_builder.header("authorization", format!("Bearer {}", self.api_key));
        }

        Ok(req_builder.send().await?)
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let mut messages = Vec::new();

        if let Some(sys) = request.full_system_prompt() {
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

        // Clones keep this callable once per setup attempt; `json!` takes
        // ownership of interpolated values.
        let build_body = |include_usage: bool| {
            let mut body = json!({
                "model": request.model.clone(),
                "messages": messages.clone(),
                "stream": true,
            });
            if include_usage {
                body["stream_options"] = json!({ "include_usage": true });
            }
            if !tools.is_empty() {
                body["tools"] = json!(tools.clone());
            }
            if let Some(temp) = request.temperature {
                body["temperature"] = json!(temp);
            }
            if let Some(max_tokens) = request.max_tokens {
                body["max_tokens"] = json!(max_tokens);
            }
            body
        };

        let url = self.chat_url();
        // `stream_options.include_usage` is best-effort: gateways that reject it
        // get one retry without it instead of a hard 400 failure. Other setup
        // failures are NOT retried here: the agent runner owns retry policy so
        // attempts stay bounded per step.
        let mut include_usage = true;
        let resp = loop {
            let body = build_body(include_usage);
            let resp = self.post_once(&url, &body).await?;
            if resp.status().is_success() {
                break resp;
            }
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            let lower = text.to_lowercase();
            let mentions_stream_options =
                lower.contains("stream_options") || lower.contains("include_usage");
            if include_usage && status == 400 && mentions_stream_options {
                include_usage = false;
                continue;
            }
            return Err(LLMError::Api { status, body: text });
        };

        let event_source = resp.bytes_stream().eventsource();

        let stream = stream::unfold(
            (event_source, StreamState::default(), VecDeque::new()),
            |(mut source, mut state, mut pending)| async move {
                loop {
                    if let Some(event) = pending.pop_front() {
                        return Some((event, (source, state, pending)));
                    }
                    let event = match source.next().await {
                        // Clean close, with or without a trailing [DONE]: end the
                        // stream instead of reporting EOF. Several gateways just
                        // close the connection after the final chunk.
                        None => return None,
                        Some(Ok(event)) => event,
                        Some(Err(err)) => {
                            // Trailing transport noise after a finished turn is
                            // not a failure: some gateways close immediately
                            // after a valid finish chunk instead of sending
                            // `[DONE]`, and reqwest can surface that close as
                            // an EOF error.
                            if state.terminal_received {
                                return None;
                            }
                            return Some((
                                Err(LLMError::Stream(err.to_string())),
                                (source, state, pending),
                            ));
                        }
                    };
                    let data = event.data.trim();
                    if data.is_empty() {
                        continue;
                    }
                    if is_stream_terminator(data) {
                        return None;
                    }
                    let value: serde_json::Value = match serde_json::from_str(data) {
                        Ok(value) => value,
                        // Lenient: gateways interleave pings and other non-JSON
                        // payloads. Skipping keeps the turn alive instead of
                        // failing it with "Invalid SSE JSON".
                        Err(_) => continue,
                    };
                    if let Some(message) = gateway_error_message(&value) {
                        return Some((Err(LLMError::Stream(message)), (source, state, pending)));
                    }
                    // `consume` records `terminal_received` when a finish chunk
                    // arrives, so EOF right after it ends the stream cleanly.
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
                source: maukode_tools::ToolSource::Native,
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

    #[tokio::test]
    async fn eof_after_finish_chunk_is_successful_termination() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let _server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            loop {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
                if request.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }

            let payload = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"done\"}}]}\n\n",
                "data: {\"choices\":[{\"finish_reason\":\"stop\"}]}\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n{:X}\r\n{}\r\n",
                payload.len(),
                payload
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            // Deliberately omit the final zero-length HTTP chunk. This reproduces gateways that
            // expose `unexpected EOF during chunk size line` after a complete SSE finish event.
        });

        let client = OpenAIClient::new("test-key").with_base_url(format!("http://{address}"));
        let request = LLMRequest::new("test-model", vec![maukode_protocol::Message::user("hello")]);
        let events = client
            .stream(request)
            .await
            .unwrap()
            .collect::<Vec<_>>()
            .await;

        assert!(events.iter().all(Result::is_ok), "{events:?}");
        assert!(events.iter().any(|event| matches!(
            event,
            Ok(LLMEvent::Finish {
                reason: FinishReason::EndTurn
            })
        )));
    }

    /// Some OpenAI-compatible gateways report `finish_reason: "stop"` even when tool calls were
    /// streamed. The tool call must still be emitted; callers must not read `Finish` as "done".
    #[test]
    fn stop_finish_reason_with_tool_calls_still_emits_the_call() {
        let mut state = StreamState::default();
        state.consume(&json!({"choices": [{"delta": {"tool_calls": [{
            "index": 0, "id": "call_1", "function": {"name": "grep", "arguments": r#"{"pattern":"x"}"#}
        }]}}]}));
        let done = state.consume(&json!({"choices": [{"finish_reason": "stop"}]}));
        assert!(matches!(
            done.as_slice(),
            [Ok(LLMEvent::ToolCallFinish { tool_call }), Ok(LLMEvent::Finish { reason: FinishReason::EndTurn })]
            if tool_call.name == "grep"
        ));
    }

    #[test]
    fn stream_helpers_are_lenient_but_surface_gateway_errors() {
        assert!(is_stream_terminator("[DONE]"));
        assert!(is_stream_terminator("[DONE]  "));
        assert!(is_stream_terminator("  [DONE]\r"));
        assert!(!is_stream_terminator("[DONE]!"));
        assert!(!is_stream_terminator(""));

        assert!(gateway_error_message(&json!({"choices": []})).is_none());
        assert_eq!(
            gateway_error_message(&json!({"error": {"message": "bad model", "code": "400"}})),
            Some("Gateway error (400): bad model".to_string())
        );
        assert_eq!(
            gateway_error_message(&json!({"error": "overloaded"})),
            Some("Gateway error: overloaded".to_string())
        );

        assert!(LLMError::Stream("EOF while streaming".into()).is_transient());
        assert!(LLMError::Stream("connection reset by peer".into()).is_transient());
        // reqwest/hyper mid-stream body cuts (e.g. llm-kita gateway closing
        // the body mid-turn) must retry instead of failing the turn.
        assert!(LLMError::Stream(
            "error decoding response body: stream closed before response completed".into()
        )
        .is_transient());
        assert!(LLMError::Stream(
            "error decoding response body: connection closed before message completed".into()
        )
        .is_transient());
        assert!(LLMError::Api {
            status: 503,
            body: String::new()
        }
        .is_transient());
        assert!(LLMError::Api {
            status: 429,
            body: String::new()
        }
        .is_transient());
        assert!(!LLMError::Api {
            status: 400,
            body: String::new()
        }
        .is_transient());
        assert!(!LLMError::Stream("boom".into()).is_transient());
        assert!(!LLMError::Stream("Tool call has no name".into()).is_transient());
    }

    async fn read_json_body(socket: &mut tokio::net::TcpStream) -> serde_json::Value {
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
        let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
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
        serde_json::from_slice(&request[header_end..header_end + body_length]).unwrap()
    }

    /// Gateways interleave empty / non-JSON SSE payloads and pad `[DONE]`.
    /// None of that may fail the turn anymore.
    #[tokio::test]
    async fn lenient_gateway_stream_skips_garbage_and_trims_done() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = read_json_body(&mut socket).await;
            assert_eq!(body["stream"], true);
            let payload = "data: \n\n\
                 data: not-json{{{\n\n\
                 data: {\"choices\": [{\"delta\": {\"content\": \"hi\"}}]}\n\n\
                 data: [DONE]   \n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n{payload}"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });

        let client = OpenAIClient::new("test-key").with_base_url(format!("http://{address}"));
        let request = LLMRequest::new("test-model", vec![maukode_protocol::Message::user("hi")]);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.stream(request).await.unwrap().collect::<Vec<_>>(),
        )
        .await;
        server.abort();
        let events = result.expect("lenient stream should end cleanly");
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Ok(LLMEvent::TextDelta { text }) if text == "hi")),
            "text chunk must survive surrounding garbage, got: {events:?}"
        );
        assert!(
            events.iter().all(|e| e.is_ok()),
            "no chunk may surface as an error, got: {events:?}"
        );
    }

    /// Gateways that reject `stream_options.include_usage` get one retry
    /// without it instead of a hard 400 failure.
    #[tokio::test]
    async fn stream_options_rejection_is_retried_without_it() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let first = read_json_body(&mut socket).await;
            assert!(first.get("stream_options").is_some());
            let rejection =
                r#"{"error":{"message":"stream_options is not supported","code":"400"}}"#;
            let response = format!(
                "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n{rejection}",
                rejection.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();

            let second = read_json_body(&mut socket).await;
            assert!(second.get("stream_options").is_none());
            let payload = "data: {\"choices\": [{\"delta\": {\"content\": \"ok\"}}]}\n\n\
                 data: [DONE]\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n{payload}"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });

        let client = OpenAIClient::new("test-key").with_base_url(format!("http://{address}"));
        let request = LLMRequest::new("test-model", vec![maukode_protocol::Message::user("hi")]);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.stream(request).await.unwrap().collect::<Vec<_>>(),
        )
        .await;
        server.abort();
        let events = result.expect("fallback request should succeed");
        assert!(events.iter().any(|e| matches!(
            e,
            Ok(LLMEvent::TextDelta { text }) if text == "ok"
        )));
    }

    /// A gateway error delivered as a 200 SSE payload must surface as an
    /// error (with its message) instead of stalling into a bare EOF.
    #[tokio::test]
    async fn sse_error_payload_surfaces_gateway_message() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let _ = read_json_body(&mut socket).await;
            let payload =
                "data: {\"error\": {\"message\": \"model overloaded\", \"code\": \"529\"}}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n{payload}"
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });

        let client = OpenAIClient::new("test-key").with_base_url(format!("http://{address}"));
        let request = LLMRequest::new("test-model", vec![maukode_protocol::Message::user("hi")]);
        let mut stream = client.stream(request).await.unwrap();
        let first = tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
            .await
            .expect("error payload should arrive promptly")
            .expect("stream should yield the gateway error");
        server.abort();
        assert!(
            matches!(&first, Err(LLMError::Stream(msg)) if msg.contains("model overloaded")),
            "gateway message must surface, got: {first:?}"
        );
    }
}
