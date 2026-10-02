use anyhow::Result;
use futures::StreamExt;
use maukode_protocol::{LLMEvent, SessionId};
use maukode_server::ServerClient;
use std::io::{self, Write};

pub async fn run_remote_turn(
    client: &ServerClient,
    session_id: SessionId,
    prompt: &str,
) -> Result<()> {
    // 1. Subscribe to SSE events
    let mut event_stream = client.subscribe_events(session_id).await?;

    // 2. Submit prompt to start the turn on the server
    client.submit_prompt(session_id, prompt).await?;

    let mut stdout = io::stdout();
    let mut in_reasoning = false;
    let mut in_assistant = false;

    // 3. Consume events in real-time
    while let Some(event_res) = event_stream.next().await {
        match event_res {
            Ok(event) => match event {
                LLMEvent::ReasoningDelta { text } => {
                    if !in_reasoning {
                        print!("\x1b[2;3m💭 Thinking: ");
                        in_reasoning = true;
                    }
                    print!("{text}");
                    stdout.flush()?;
                }
                LLMEvent::TextDelta { text } => {
                    if in_reasoning {
                        println!("\x1b[0m\n");
                        in_reasoning = false;
                    }
                    if !in_assistant {
                        print!("● ");
                        in_assistant = true;
                    }
                    print!("{text}");
                    stdout.flush()?;
                }
                LLMEvent::ToolCallStart { name, id } => {
                    if in_reasoning {
                        println!("\x1b[0m\n");
                        in_reasoning = false;
                    }
                    println!("\n⚡ \x1b[1;35mTool Call:\x1b[0m \x1b[1m{name}\x1b[0m \x1b[2m({id})\x1b[0m");
                }
                LLMEvent::ToolCallFinish { tool_call } => {
                    if !tool_call.arguments.is_null() && tool_call.arguments != "" {
                        println!("  \x1b[2mArgs: {}\x1b[0m", tool_call.arguments);
                    }
                }
                LLMEvent::ToolResult(tr) => {
                    if tr.is_error {
                        println!("  ✗ \x1b[1;31mError:\x1b[0m {}", tr.content);
                    } else {
                        let preview = if tr.content.len() > 300 {
                            let end = tr.content.floor_char_boundary(300);
                            format!("{}... (truncated)", &tr.content[..end])
                        } else {
                            tr.content
                        };
                        for line in preview.lines() {
                            println!("  \x1b[2m│ {line}\x1b[0m");
                        }
                    }
                }
                LLMEvent::Usage(usage) => {
                    println!("\n\x1b[2m[Tokens: in={}, out={}]\x1b[0m", usage.input_tokens, usage.output_tokens);
                }
                LLMEvent::ContextWarning { current_tokens, max_tokens, percent, message } => {
                    println!("\n\x1b[1;33m⚠️  [CONTEXT WARNING]\x1b[0m {message} ({current_tokens}/{max_tokens} tokens, {percent}%)");
                }
                LLMEvent::PermissionRequest(req) => {
                    let cmd = req.command.as_deref().unwrap_or(req.path.as_deref().unwrap_or(&req.description));
                    println!("\n\x1b[1;33m⚠️  [CONFIRMATION REQUIRED]\x1b[0m");
                    println!("  Tool: \x1b[1m{}\x1b[0m", req.tool_name);
                    println!("  Command: \x1b[1;37m{}\x1b[0m", cmd);
                    println!("  Risk: \x1b[1;31m{}\x1b[0m", req.description);
                    print!("  Allow execution? [y: once, a: session, n: reject]: ");
                    stdout.flush()?;

                    let mut answer = String::new();
                    let decision = if io::stdin().read_line(&mut answer).is_ok() {
                        match answer.trim().to_lowercase().as_str() {
                            "y" | "yes" => maukode_protocol::PermissionDecision::AllowOnce,
                            "a" | "all" => maukode_protocol::PermissionDecision::AllowSession,
                            _ => maukode_protocol::PermissionDecision::Deny,
                        }
                    } else {
                        maukode_protocol::PermissionDecision::Deny
                    };

                    let _ = client.respond_permission(session_id, &req.request_id, decision).await;
                }
                LLMEvent::Finish { .. } => {
                    break;
                }
                LLMEvent::Error { message } => {
                    eprintln!("\n\x1b[1;31mServer Error:\x1b[0m {message}");
                    break;
                }
                _ => {}
            },
            Err(e) => {
                eprintln!("\n\x1b[1;31mConnection Error:\x1b[0m {e}");
                break;
            }
        }
    }

    println!();
    Ok(())
}
