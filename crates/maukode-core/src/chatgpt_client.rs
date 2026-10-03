//! Responses API adapter for ChatGPT plan usage.
//! See https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference

use crate::chatgpt_auth::ChatGptAuth;
use async_trait::async_trait;
use eventsource_stream::Eventsource;
use futures::{stream, StreamExt};
use maukode_llm::{LLMClient, LLMError, LLMRequest, LLMStream};
use maukode_protocol::{ContentPart, FinishReason, LLMEvent, Role, TokenUsage, ToolCall};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

pub struct ChatGptClient {
    auth: ChatGptAuth,
    http: reqwest::Client,
    responses_url: String,
}

impl ChatGptClient {
    pub fn new(auth: ChatGptAuth) -> Self {
        Self {
            auth,
            http: reqwest::Client::new(),
            responses_url: "https://api.openai.com/v1/responses".into(),
        }
    }

    #[cfg(test)]
    fn with_responses_url(mut self, url: String) -> Self {
        self.responses_url = url;
        self
    }
}

fn build_body(request: &LLMRequest) -> Value {
    let mut input = Vec::new();
    let mut instructions = request.system_prompt.clone().unwrap_or_default();
    for message in &request.messages {
        for part in &message.parts {
            if let ContentPart::ProviderItem { value } = part {
                input.push(value.clone());
            }
        }
        let text = message.text_content();
        if !text.is_empty() {
            match message.role {
                Role::System => {
                    if !instructions.is_empty() {
                        instructions.push_str("\n\n");
                    }
                    instructions.push_str(&text);
                }
                Role::User | Role::Assistant => input.push(json!({
                    "role": if message.role == Role::User { "user" } else { "assistant" },
                    "content": text,
                })),
            }
        }
        for part in &message.parts {
            match part {
                ContentPart::Text { .. }
                | ContentPart::Reasoning { .. }
                | ContentPart::ProviderItem { .. } => {}
                ContentPart::ToolCall(call) => input.push(json!({
                    "type": "function_call",
                    "call_id": call.id,
                    "name": call.name,
                    "namespace": "maukode",
                    "arguments": call.arguments.to_string(),
                })),
                ContentPart::ToolResult(result) => input.push(json!({
                    "type": "function_call_output",
                    "call_id": result.tool_call_id,
                    "output": result.content,
                })),
            }
        }
    }
    let mut body = json!({
        "model": request.model,
        "input": input,
        "store": false,
        "stream": true,
    });
    body["include"] = json!(["reasoning.encrypted_content"]);
    if !instructions.is_empty() {
        body["instructions"] = json!(instructions);
    }
    if !request.tools.is_empty() {
        let tools: Vec<_> = request
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                })
            })
            .collect();
        body["tools"] = json!([{
            "type": "namespace",
            "name": "maukode",
            "description": "Tools for reading, searching, and editing the current workspace.",
            "tools": tools,
        }]);
    }
    body
}

#[derive(Default)]
struct ResponseState {
    pending: VecDeque<Result<LLMEvent, LLMError>>,
    call_ids: HashMap<String, String>,
    saw_tool: bool,
    completed: bool,
}

impl ResponseState {
    fn consume(&mut self, value: &Value) {
        let kind = value["type"].as_str().unwrap_or_default();
        match kind {
            "response.output_text.delta" => {
                if let Some(text) = value["delta"].as_str() {
                    self.pending
                        .push_back(Ok(LLMEvent::TextDelta { text: text.into() }));
                }
            }
            "response.output_item.added" if value["item"]["type"] == "function_call" => {
                let item = &value["item"];
                if let (Some(id), Some(name)) = (item["call_id"].as_str(), item["name"].as_str()) {
                    if let Some(item_id) = item["id"].as_str() {
                        self.call_ids.insert(item_id.into(), id.into());
                    }
                    self.pending.push_back(Ok(LLMEvent::ToolCallStart {
                        id: id.into(),
                        name: local_tool_name(name).into(),
                    }));
                }
            }
            "response.function_call_arguments.delta" => {
                if let (Some(id), Some(delta)) =
                    (value["item_id"].as_str(), value["delta"].as_str())
                {
                    self.pending.push_back(Ok(LLMEvent::ToolCallDelta {
                        id: self.call_ids.get(id).cloned().unwrap_or_else(|| id.into()),
                        delta: delta.into(),
                    }));
                }
            }
            "response.output_item.done" if value["item"]["type"] == "function_call" => {
                let item = &value["item"];
                let (Some(id), Some(name), Some(arguments)) = (
                    item["call_id"].as_str(),
                    item["name"].as_str(),
                    item["arguments"].as_str(),
                ) else {
                    self.pending
                        .push_back(Err(LLMError::Stream("Incomplete function call".into())));
                    return;
                };
                match serde_json::from_str(arguments) {
                    Ok(arguments) => {
                        self.saw_tool = true;
                        self.pending.push_back(Ok(LLMEvent::ToolCallFinish {
                            tool_call: ToolCall {
                                id: id.into(),
                                name: local_tool_name(name).into(),
                                arguments,
                            },
                        }));
                    }
                    Err(err) => self.pending.push_back(Err(LLMError::Stream(format!(
                        "Invalid function arguments: {err}"
                    )))),
                }
            }
            "response.output_item.done" if value["item"]["type"] == "reasoning" => {
                self.pending.push_back(Ok(LLMEvent::ProviderItem {
                    item: value["item"].clone(),
                }));
            }
            "response.completed" => {
                self.completed = true;
                let usage = &value["response"]["usage"];
                if let (Some(input_tokens), Some(output_tokens)) = (
                    usage["input_tokens"].as_u64(),
                    usage["output_tokens"].as_u64(),
                ) {
                    self.pending.push_back(Ok(LLMEvent::Usage(TokenUsage {
                        input_tokens,
                        output_tokens,
                        cache_read_tokens: usage["input_tokens_details"]["cached_tokens"].as_u64(),
                        cache_write_tokens: None,
                    })));
                }
                self.pending.push_back(Ok(LLMEvent::Finish {
                    reason: if self.saw_tool {
                        FinishReason::ToolUse
                    } else {
                        FinishReason::EndTurn
                    },
                }));
            }
            "response.failed" | "response.incomplete" | "error" => {
                self.completed = true;
                let error = if value["error"].is_object() {
                    &value["error"]
                } else {
                    &value["response"]["error"]
                };
                let code = error["code"].as_str();
                let reason = error["message"]
                    .as_str()
                    .or_else(|| value["response"]["incomplete_details"]["reason"].as_str())
                    .or_else(|| value["message"].as_str())
                    .unwrap_or(kind);
                let detail = match code {
                    Some(code @ "subscription_sharing_usage_limit_exceeded") => format!(
                        "ChatGPT app usage limit reached ({code}). Check ChatGPT Settings → Usage, retry when usage is available, or choose an API-key provider. {reason}"
                    ),
                    Some(code) => format!("{code}: {reason}"),
                    None => reason.to_string(),
                };
                self.pending.push_back(Err(LLMError::Stream(detail)));
            }
            _ => {}
        }
    }
}

fn local_tool_name(name: &str) -> &str {
    name.strip_prefix("maukode.").unwrap_or(name)
}

#[async_trait]
impl LLMClient for ChatGptClient {
    async fn stream(&self, request: LLMRequest) -> Result<LLMStream, LLMError> {
        let token = self
            .auth
            .access_token()
            .await
            .map_err(|err| LLMError::Config(err.to_string()))?;
        let response = self
            .http
            .post(&self.responses_url)
            .bearer_auth(token)
            .json(&build_body(&request))
            .send()
            .await?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            let body = response.text().await.unwrap_or_default();
            return Err(LLMError::Api { status, body });
        }
        let source = response.bytes_stream().eventsource();
        let stream = stream::unfold(
            (source, ResponseState::default()),
            |(mut source, mut state)| async move {
                loop {
                    if let Some(event) = state.pending.pop_front() {
                        return Some((event, (source, state)));
                    }
                    if state.completed {
                        return None;
                    }
                    let event = match source.next().await {
                        Some(Ok(event)) => event,
                        Some(Err(err)) => {
                            return Some((Err(LLMError::Stream(err.to_string())), (source, state)))
                        }
                        None if state.completed => return None,
                        None => {
                            return Some((
                                Err(LLMError::Stream(
                                    "Response stream ended before completion".into(),
                                )),
                                (
                                    source,
                                    ResponseState {
                                        completed: true,
                                        ..state
                                    },
                                ),
                            ))
                        }
                    };
                    if event.data == "[DONE]" {
                        return None;
                    }
                    match serde_json::from_str::<Value>(&event.data) {
                        Ok(value) => state.consume(&value),
                        Err(err) => state.pending.push_back(Err(LLMError::Stream(format!(
                            "Invalid response event: {err}"
                        )))),
                    }
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn request_uses_plan_compatible_responses_fields_and_tool_history() {
        let request = LLMRequest::new(
            "model",
            vec![
                Message::user("Find it"),
                Message::assistant(vec![
                    ContentPart::ProviderItem {
                        value: json!({"type":"reasoning","encrypted_content":"opaque"}),
                    },
                    ContentPart::ToolCall(ToolCall {
                        id: "call_1".into(),
                        name: "grep".into(),
                        arguments: json!({"pattern":"x"}),
                    }),
                ]),
                Message {
                    role: Role::User,
                    parts: vec![ContentPart::ToolResult(maukode_protocol::ToolResult {
                        tool_call_id: "call_1".into(),
                        tool_name: "grep".into(),
                        content: "found".into(),
                        is_error: false,
                    })],
                },
            ],
        )
        .with_system_prompt("Be helpful");
        let body = build_body(&request);
        assert_eq!(body["store"], false);
        assert_eq!(body["stream"], true);
        assert_eq!(body["instructions"], "Be helpful");
        assert_eq!(body["include"][0], "reasoning.encrypted_content");
        assert_eq!(body["input"][1]["type"], "reasoning");
        assert_eq!(body["input"][2]["type"], "function_call");
        assert_eq!(body["input"][2]["name"], "grep");
        assert_eq!(body["input"][2]["namespace"], "maukode");
        assert_eq!(body["input"][3]["type"], "function_call_output");
        assert!(body.get("temperature").is_none());
        assert!(body.get("max_output_tokens").is_none());
    }

    #[test]
    fn response_stream_maps_function_call_and_completion() {
        let mut state = ResponseState::default();
        state.consume(&json!({"type":"response.output_item.added","item":{
            "type":"function_call","call_id":"call_1","name":"grep","namespace":"maukode"
        }}));
        state.consume(&json!({"type":"response.output_item.done","item":{
            "type":"function_call","call_id":"call_1","name":"grep","namespace":"maukode","arguments":"{}"
        }}));
        state.consume(&json!({"type":"response.output_item.done","item":{
            "type":"reasoning","encrypted_content":"opaque"
        }}));
        state.consume(&json!({"type":"response.completed","response":{"usage":{
            "input_tokens":10,"output_tokens":5
        }}}));
        assert!(
            matches!(state.pending.pop_front(), Some(Ok(LLMEvent::ToolCallStart { name, .. })) if name == "grep")
        );
        assert!(
            matches!(state.pending.pop_front(), Some(Ok(LLMEvent::ToolCallFinish { tool_call })) if tool_call.name == "grep")
        );
        assert!(
            matches!(state.pending.pop_front(), Some(Ok(LLMEvent::ProviderItem { item })) if item["encrypted_content"] == "opaque")
        );
        assert!(matches!(
            state.pending.pop_front(),
            Some(Ok(LLMEvent::Usage(_)))
        ));
        assert!(matches!(
            state.pending.pop_front(),
            Some(Ok(LLMEvent::Finish {
                reason: FinishReason::ToolUse
            }))
        ));
    }

    #[test]
    fn streamed_plan_limit_keeps_code_and_recovery() {
        let mut state = ResponseState::default();
        state.consume(&json!({
            "type": "error",
            "error": {
                "code": "subscription_sharing_usage_limit_exceeded",
                "message": "The app usage limit was reached"
            }
        }));
        let Some(Err(LLMError::Stream(message))) = state.pending.pop_front() else {
            panic!("expected streaming error");
        };
        assert!(message.contains("subscription_sharing_usage_limit_exceeded"));
        assert!(message.contains("ChatGPT Settings → Usage"));
        assert!(message.contains("The app usage limit was reached"));
    }

    #[tokio::test]
    async fn local_responses_stream_uses_oauth_bearer_and_completes() {
        let temp = tempfile::tempdir().unwrap();
        let auth_path = temp.path().join("auth.json");
        std::fs::write(&auth_path, serde_json::to_vec(&json!({
            "host_id": "urn:uuid:test", "active_client_id": "oaiapp_test", "profiles": [{
                "email": "test@example.com", "subject": "subject", "client_id": "oaiapp_test",
                "id_token": "unused", "access_token": "fixture-access", "refresh_token": "unused",
                "scope": "chatgpt.tokens.use.direct", "expires_at": u64::MAX,
            }]
        })).unwrap()).unwrap();
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
            let headers = String::from_utf8_lossy(&request[..header_end]).to_ascii_lowercase();
            assert!(headers.starts_with("post /v1/responses http/1.1"));
            assert!(headers.contains("authorization: bearer fixture-access"));
            let length: usize = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("content-length:")
                        .and_then(|value| value.trim().parse().ok())
                })
                .unwrap();
            while request.len() - header_end < length {
                let mut chunk = [0; 1024];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&chunk[..count]);
            }
            let body: Value =
                serde_json::from_slice(&request[header_end..header_end + length]).unwrap();
            assert_eq!(body["store"], false);
            assert_eq!(body["stream"], true);
            assert_eq!(body["tools"][0]["type"], "namespace");
            let payload = [
                json!({"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"grep","namespace":"maukode"}}),
                json!({"type":"response.output_item.done","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"grep","namespace":"maukode","arguments":"{}"}}),
                json!({"type":"response.completed","response":{"usage":{"input_tokens":3,"output_tokens":2}}}),
            ].iter().map(|event| format!("data: {event}\n\n")).collect::<String>();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\n{payload}");
            socket.write_all(response.as_bytes()).await.unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        });
        let client = ChatGptClient::new(ChatGptAuth::at_path(auth_path))
            .with_responses_url(format!("http://{address}/v1/responses"));
        let request = LLMRequest::new("test-model", vec![Message::user("find")]).with_tools(vec![
            maukode_tools::ToolDefinition {
                name: "grep".into(),
                description: "Search files".into(),
                parameters: json!({"type":"object"}),
            },
        ]);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            client.stream(request).await.unwrap().collect::<Vec<_>>(),
        )
        .await;
        server.abort();
        let events = result.expect("response.completed should end the stream before socket close");
        assert!(events.iter().any(|event| matches!(event, Ok(LLMEvent::ToolCallFinish { tool_call }) if tool_call.name == "grep")));
        assert!(events.iter().any(|event| matches!(
            event,
            Ok(LLMEvent::Finish {
                reason: FinishReason::ToolUse
            })
        )));
    }
}
