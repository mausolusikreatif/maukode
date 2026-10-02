use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use clap::{Parser, Subcommand};
use maukode_core::{ContextManager, InMemorySessionStore, RunnerConfig, Session, SessionRunner, SessionStore};
use maukode_llm::{AnthropicClient, LLMClient, OpenAIClient};
use maukode_protocol::LLMEvent;
use maukode_tools::create_default_registry;
use tokio::sync::mpsc;

#[derive(Parser, Debug)]
#[command(name = "maukode", author, version, about = "Agentic AI coding assistant in Rust (inspired by OpenCode)")]
struct Cli {
    #[arg(short, long, help = "Model provider: 'anthropic' or 'openai' (or compatible)")]
    provider: Option<String>,

    #[arg(short, long, help = "Model identifier (e.g. claude-3-7-sonnet-20250219, gpt-4o)")]
    model: Option<String>,

    #[arg(long, help = "API key (defaults to ANTHROPIC_API_KEY or OPENAI_API_KEY)")]
    api_key: Option<String>,

    #[arg(long, help = "Custom base URL for the LLM API")]
    base_url: Option<String>,

    #[arg(short = 'C', long, help = "Working directory (defaults to current dir)")]
    cwd: Option<PathBuf>,

    #[arg(long, default_value_t = 25, help = "Maximum turns of tool execution loop")]
    max_steps: usize,

    #[command(subcommand)]
    command: Option<Commands>,

    #[arg(trailing_var_arg = true, help = "Optional direct prompt (if no subcommand given)")]
    prompt: Vec<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    #[command(about = "Run a single prompt and exit when done")]
    Run {
        #[arg(required = true, help = "The prompt instruction for MauKode")]
        prompt: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let cwd = cli.cwd.unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let provider = cli
        .provider
        .unwrap_or_else(|| "anthropic".to_string())
        .to_lowercase();

    let (llm_client, default_model): (Arc<dyn LLMClient>, String) = match provider.as_str() {
        "anthropic" => {
            let key = cli
                .api_key
                .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                .unwrap_or_default();
            let mut client = AnthropicClient::new(key);
            if let Some(ref base) = cli.base_url {
                client = client.with_base_url(base);
            }
            (Arc::new(client), "claude-3-7-sonnet-20250219".to_string())
        }
        "openai" | "ollama" | "openrouter" => {
            let key = cli
                .api_key
                .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                .unwrap_or_default();
            let mut client = OpenAIClient::new(key);
            if let Some(ref base) = cli.base_url {
                client = client.with_base_url(base);
            }
            (Arc::new(client), "gpt-4o".to_string())
        }
        other => anyhow::bail!("Unsupported provider '{other}'. Supported: 'anthropic', 'openai'"),
    };

    let model = cli.model.unwrap_or(default_model);

    let tools = Arc::new(create_default_registry(&cwd));
    let context = Arc::new(ContextManager::new(&cwd));
    let store = InMemorySessionStore::new();
    let session = store.save(Session::new()).await;

    let config = RunnerConfig {
        model: model.clone(),
        max_steps: cli.max_steps,
        max_tokens: Some(4096),
        temperature: None,
    };

    let runner = SessionRunner::new(
        Arc::clone(&session),
        llm_client,
        tools,
        context,
        config,
    );

    // Determine initial prompt
    let prompt_to_run = match cli.command {
        Some(Commands::Run { prompt }) => Some(prompt),
        None => {
            if !cli.prompt.is_empty() {
                Some(cli.prompt.join(" "))
            } else {
                None
            }
        }
    };

    if let Some(prompt) = prompt_to_run {
        println!("🚀 \x1b[1;36mMauKode\x1b[0m running with model \x1b[1;33m{}\x1b[0m", model);
        println!("📂 Workspace: \x1b[2m{}\x1b[0m\n", cwd.display());
        execute_turn(&runner, Some(prompt)).await?;
        return Ok(());
    }

    // Interactive REPL loop
    println!("══════════════════════════════════════════════════════════════════");
    println!("  🤖 \x1b[1;36mMauKode\x1b[0m - AI Coding Agent (Rust Edition)");
    println!("  Model: \x1b[1;33m{}\x1b[0m | Workspace: \x1b[2m{}\x1b[0m", model, cwd.display());
    println!("  Type 'exit' or 'quit' to end the session.");
    println!("══════════════════════════════════════════════════════════════════\n");

    loop {
        print!("\x1b[1;32mmaukode>\x1b[0m ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            break;
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            println!("Goodbye!");
            break;
        }

        println!();
        execute_turn(&runner, Some(trimmed.to_string())).await?;
        println!("\n");
    }

    Ok(())
}

async fn execute_turn(runner: &SessionRunner, prompt: Option<String>) -> anyhow::Result<()> {
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<LLMEvent>();

    let runner_task = {
        let runner = runner;
        async move { runner.run(prompt, event_tx).await }
    };

    let display_task = async {
        let mut stdout = io::stdout();
        let mut in_reasoning = false;

        while let Some(event) = event_rx.recv().await {
            match event {
                LLMEvent::TextDelta { text } => {
                    if in_reasoning {
                        print!("\x1b[0m\n\n");
                        in_reasoning = false;
                    }
                    print!("{text}");
                    stdout.flush().ok();
                }
                LLMEvent::ReasoningDelta { text } => {
                    if !in_reasoning {
                        print!("\x1b[2m\x1b[3m[Thinking] ");
                        in_reasoning = true;
                    }
                    print!("{text}");
                    stdout.flush().ok();
                }
                LLMEvent::ToolCallStart { name, id } => {
                    if in_reasoning {
                        print!("\x1b[0m\n\n");
                        in_reasoning = false;
                    }
                    println!("\n\x1b[1;35m⚡ Tool Call: \x1b[1;37m{name}\x1b[0m \x1b[2m({id})\x1b[0m");
                }
                LLMEvent::ToolCallFinish { tool_call } => {
                    println!("\x1b[2m  Args: {}\x1b[0m", tool_call.arguments);
                }
                LLMEvent::Usage(usage) => {
                    println!(
                        "\n\x1b[2m[Tokens: In {} | Out {}{}]\x1b[0m",
                        usage.input_tokens,
                        usage.output_tokens,
                        usage
                            .cache_read_tokens
                            .map(|c| format!(" | Cache Read {c}"))
                            .unwrap_or_default()
                    );
                }
                LLMEvent::Error { message } => {
                    eprintln!("\n\x1b[1;31m❌ Error: {message}\x1b[0m");
                }
                _ => {}
            }
        }
    };

    let (run_res, _) = tokio::join!(runner_task, display_task);
    run_res?;
    Ok(())
}
