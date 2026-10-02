use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use clap::{Parser, Subcommand};
use maukode_core::{
    AppConfig, ContextManager, ProviderConfig, ProviderKind, RunnerConfig, Session, SessionRunner,
    SqliteSessionStore,
};
use maukode_protocol::{LLMEvent, SessionId};
use maukode_tools::create_default_registry;
use tokio::sync::mpsc;

mod remote;

#[derive(Parser, Debug)]
#[command(name = "maukode", author, version, about = "Agentic AI coding assistant in Rust (inspired by OpenCode)")]
struct Cli {
    #[arg(short, long, help = "Model provider: 'anthropic', 'openai', 'ollama', etc.")]
    provider: Option<String>,

    #[arg(short, long, help = "Model identifier (e.g. claude-3-7-sonnet-20250219, gpt-4o, qwen2.5-coder)")]
    model: Option<String>,

    #[arg(long, help = "API key (defaults to ANTHROPIC_API_KEY or OPENAI_API_KEY)")]
    api_key: Option<String>,

    #[arg(long, help = "Custom base URL for the LLM API")]
    base_url: Option<String>,

    #[arg(short = 'C', long, help = "Working directory (defaults to current dir)")]
    cwd: Option<PathBuf>,

    #[arg(short = 's', long, help = "Resume a specific session by UUID")]
    session: Option<String>,

    #[arg(short = 'c', long, help = "Continue the latest session")]
    continue_session: bool,

    #[arg(long, default_value_t = 25, help = "Maximum turns of tool execution loop")]
    max_steps: usize,

    #[arg(long, help = "Run in simple command-line REPL mode instead of full TUI")]
    cli: bool,

    #[arg(long, help = "Remote server URL (e.g. http://vps:4096) to connect to instead of local in-process")]
    remote: Option<String>,

    #[arg(long, env = "MAUKODE_AUTH_TOKEN", help = "Authentication Bearer token for remote server")]
    token: Option<String>,

    #[arg(long, help = "Run in standalone in-process mode without local background daemon")]
    standalone: bool,

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
    #[command(about = "Manage background server daemon or start foreground server")]
    Server {
        #[command(subcommand)]
        action: Option<ServerAction>,

        #[arg(short, long, default_value = "127.0.0.1:4096", help = "Socket address to bind foreground server to (e.g. 0.0.0.0:4096)")]
        bind: String,

        #[arg(short, long, env = "MAUKODE_AUTH_TOKEN", help = "Secret Bearer token required for clients")]
        token: Option<String>,

        #[arg(short = 'd', long, help = "Default root directory for workspaces")]
        dir: Option<PathBuf>,
    },
}

#[derive(Subcommand, Debug)]
enum ServerAction {
    #[command(about = "Start server daemon in the background")]
    Start {
        #[arg(short, long, default_value_t = 4096, help = "Port to listen on")]
        port: u16,

        #[arg(short, long, env = "MAUKODE_AUTH_TOKEN", help = "Secret Bearer token")]
        token: Option<String>,
    },
    #[command(about = "Stop the background server daemon")]
    Stop,
    #[command(about = "Show status of the background server daemon")]
    Status,
    #[command(about = "Restart the background server daemon")]
    Restart {
        #[arg(short, long, default_value_t = 4096, help = "Port to listen on")]
        port: u16,

        #[arg(short, long, env = "MAUKODE_AUTH_TOKEN", help = "Secret Bearer token")]
        token: Option<String>,
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

    // Handle "server" subcommand
    if let Some(Commands::Server { action, bind, token, dir }) = cli.command {
        match action {
            Some(ServerAction::Start { port, token }) => {
                let info = maukode_server::ensure_daemon_running(port, token).await?;
                println!("🚀 MauKode daemon started in background");
                println!("  URL: \x1b[1;32m{}\x1b[0m", info.url);
                println!("  PID: {}", info.pid);
                println!("  Log: \x1b[2m{}\x1b[0m", maukode_server::daemon::daemon_log_path().display());
                return Ok(());
            }
            Some(ServerAction::Stop) => {
                if maukode_server::stop_daemon().await? {
                    println!("🛑 MauKode daemon stopped.");
                } else {
                    println!("ℹ️ No active MauKode daemon was running.");
                }
                return Ok(());
            }
            Some(ServerAction::Status) => {
                if let Some(info) = maukode_server::get_daemon_status().await? {
                    println!("● \x1b[1;32mMauKode daemon is running\x1b[0m");
                    println!("  URL: \x1b[1;32m{}\x1b[0m", info.url);
                    println!("  PID: {}", info.pid);
                    println!("  Log: \x1b[2m{}\x1b[0m", maukode_server::daemon::daemon_log_path().display());
                } else {
                    println!("○ \x1b[1;33mMauKode daemon is stopped\x1b[0m");
                }
                return Ok(());
            }
            Some(ServerAction::Restart { port, token }) => {
                let _ = maukode_server::stop_daemon().await;
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                let info = maukode_server::ensure_daemon_running(port, token).await?;
                println!("🚀 MauKode daemon restarted in background");
                println!("  URL: \x1b[1;32m{}\x1b[0m", info.url);
                println!("  PID: {}", info.pid);
                return Ok(());
            }
            None => {
                // Foreground server (e.g. for VPS)
                let bind_addr: std::net::SocketAddr = bind
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Invalid bind address '{bind}': {e}"))?;

                println!("══════════════════════════════════════════════════════════════════");
                println!("  🚀 \x1b[1;36mMauKode Server\x1b[0m (Foreground Daemon for VPS / Local)");
                println!("  Listening on: \x1b[1;32mhttp://{}\x1b[0m", bind_addr);
                if token.is_some() {
                    println!("  Authentication: \x1b[1;33mBearer Token Enabled\x1b[0m");
                } else {
                    println!("  Authentication: \x1b[1;31mDisabled (Open Access)\x1b[0m");
                }
                let workspace_dir = dir.as_deref().unwrap_or(&cwd);
                println!("  Default workspace: \x1b[2m{}\x1b[0m", workspace_dir.display());
                println!("══════════════════════════════════════════════════════════════════\n");

                let server_config = maukode_server::ServerConfig {
                    bind_addr,
                    auth_token: token,
                    default_dir: dir.or(Some(cwd)),
                };

                maukode_server::run_server(server_config).await?;
                return Ok(());
            }
        }
    }

    // Determine direct prompt
    let prompt_to_run = match cli.command {
        Some(Commands::Run { prompt }) => Some(prompt),
        None => {
            if !cli.prompt.is_empty() {
                Some(cli.prompt.join(" "))
            } else {
                None
            }
        }
        _ => None,
    };

    let parsed_session_id = if let Some(ref s) = cli.session {
        let u = uuid::Uuid::parse_str(s)
            .map_err(|e| anyhow::anyhow!("Invalid session UUID '{s}': {e}"))?;
        Some(SessionId(u))
    } else {
        None
    };

    // Determine target server URL: explicit --remote, or auto-started local daemon
    let remote_url_to_use = if let Some(ref remote_url) = cli.remote {
        Some((remote_url.clone(), cli.token.clone()))
    } else if !cli.standalone {
        match maukode_server::ensure_daemon_running(4096, None).await {
            Ok(info) => Some((info.url, info.token)),
            Err(e) => {
                tracing::warn!("Could not start local daemon: {e}. Falling back to in-process mode.");
                None
            }
        }
    } else {
        None
    };

    if let Some((url, token)) = remote_url_to_use {
        let client = Arc::new(maukode_server::ServerClient::new(&url, token)?);
        let is_remote_vps = cli.remote.is_some();
        let label = if is_remote_vps { "remote server" } else { "local daemon" };

        let target_session_id = if let Some(id) = parsed_session_id {
            id
        } else if cli.continue_session {
            let sessions = client.list_sessions(Some(&cwd), false).await.unwrap_or_default();
            if let Some(first) = sessions.first() {
                first.id
            } else {
                client.create_session(Some(&cwd), None).await?.id
            }
        } else {
            client.create_session(Some(&cwd), None).await?.id
        };

        if let Some(prompt) = prompt_to_run {
            println!("🚀 \x1b[1;36mMauKode\x1b[0m running via {label} (\x1b[2m{}\x1b[0m)", client.base_url());
            println!("📂 Workspace: \x1b[2m{}\x1b[0m\n", cwd.display());
            remote::run_remote_turn(&client, target_session_id, &prompt).await?;
            return Ok(());
        }

        if cli.cli {
            println!("══════════════════════════════════════════════════════════════════");
            println!("  🤖 \x1b[1;36mMauKode\x1b[0m (Connected to {label} at {})", client.base_url());
            println!("  Workspace: \x1b[2m{}\x1b[0m", cwd.display());
            println!("  Type 'exit' to quit.");
            println!("══════════════════════════════════════════════════════════════════\n");
            loop {
                print!("❯ ");
                io::stdout().flush()?;
                let mut line = String::new();
                io::stdin().read_line(&mut line)?;
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed == "exit" || trimmed == "quit" {
                    break;
                }
                println!();
                remote::run_remote_turn(&client, target_session_id, trimmed).await?;
                println!();
            }
            return Ok(());
        }

        // Launch full Interactive TUI connected to daemon / server!
        maukode_tui::run_tui(cwd, Some(target_session_id), cli.continue_session, Some(client)).await?;
        return Ok(());
    }

    let mut config = AppConfig::load();

    // Override config with CLI flags if provided
    if let Some(ref p) = cli.provider {
        if let Some(prov_id) = config.get_provider(p).map(|pr| pr.id.clone()) {
            config.set_active_provider(&prov_id)?;
        } else {
            // New provider specified on CLI
            let base = cli.base_url.clone().unwrap_or_else(|| "https://api.openai.com".to_string());
            let prov = ProviderConfig {
                id: p.to_lowercase(),
                name: p.clone(),
                kind: ProviderKind::OpenAiCompatible,
                base_url: base,
                api_key: cli.api_key.clone(),
                default_model: cli.model.clone(),
            };
            config.add_or_update_provider(prov);
            config.set_active_provider(p)?;
        }
    }
    if let Some(ref m) = cli.model {
        config.set_active_model(m);
    }

    let store = SqliteSessionStore::new(SqliteSessionStore::default_db_path())?;

    // If single prompt is supplied, run headless turn and persist
    if let Some(prompt) = prompt_to_run {
        println!("🚀 \x1b[1;36mMauKode\x1b[0m running with provider \x1b[1;32m{}\x1b[0m, model \x1b[1;33m{}\x1b[0m", config.active_provider, config.active_model);
        println!("📂 Workspace: \x1b[2m{}\x1b[0m\n", cwd.display());

        let active_sess = if let Some(id) = parsed_session_id {
            store.load_session(id).await?.unwrap_or_else(|| Session::new(&cwd))
        } else if cli.continue_session {
            store.get_latest_session_in_directory(&cwd).await?.unwrap_or_else(|| Session::new(&cwd))
        } else {
            Session::new(&cwd)
        };

        let session_arc = Arc::new(tokio::sync::RwLock::new(active_sess));
        let tools = Arc::new(create_default_registry(&cwd));
        let context = Arc::new(ContextManager::new(&cwd));
        let llm_client = config.create_active_llm_client()?;

        let runner_config = RunnerConfig {
            model: config.active_model.clone(),
            max_steps: cli.max_steps,
            max_tokens: Some(4096),
            temperature: None,
        };

        let runner = SessionRunner::new(
            Arc::clone(&session_arc),
            llm_client,
            tools,
            context,
            runner_config,
        );

        execute_turn(&runner, Some(prompt)).await?;

        // Save session to SQLite
        let guard = session_arc.read().await;
        store.save_session(&guard, &config.active_provider, &config.active_model).await?;
        println!("\n\x1b[2m[Session saved to SQLite: {}]\x1b[0m", guard.id);
        return Ok(());
    }

    // If CLI fallback mode is requested
    if cli.cli {
        println!("══════════════════════════════════════════════════════════════════");
        println!("  🤖 \x1b[1;36mMauKode\x1b[0m - AI Coding Agent (CLI Mode)");
        println!("  Provider: \x1b[1;32m{}\x1b[0m | Model: \x1b[1;33m{}\x1b[0m", config.active_provider, config.active_model);
        println!("  Type 'exit' to quit.");
        println!("══════════════════════════════════════════════════════════════════\n");

        let active_sess = if let Some(id) = parsed_session_id {
            store.load_session(id).await?.unwrap_or_else(|| Session::new(&cwd))
        } else if cli.continue_session {
            store.get_latest_session_in_directory(&cwd).await?.unwrap_or_else(|| Session::new(&cwd))
        } else {
            Session::new(&cwd)
        };

        let session_arc = Arc::new(tokio::sync::RwLock::new(active_sess));
        let tools = Arc::new(create_default_registry(&cwd));
        let context = Arc::new(ContextManager::new(&cwd));
        let llm_client = config.create_active_llm_client()?;

        let runner = SessionRunner::new(
            Arc::clone(&session_arc),
            llm_client,
            tools,
            context,
            RunnerConfig {
                model: config.active_model.clone(),
                max_steps: cli.max_steps,
                max_tokens: Some(4096),
                temperature: None,
            },
        );

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
                break;
            }

            println!();
            execute_turn(&runner, Some(trimmed.to_string())).await?;
            let guard = session_arc.read().await;
            store.save_session(&guard, &config.active_provider, &config.active_model).await?;
            println!("\n");
        }
        return Ok(());
    }

    // Default: Launch the full Interactive TUI in standalone mode!
    maukode_tui::run_tui(cwd, parsed_session_id, cli.continue_session, None).await?;

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
                LLMEvent::ToolResult(tr) => {
                    let badge = if tr.is_error {
                        "\x1b[1;31m✗ Tool Error:\x1b[0m"
                    } else {
                        "\x1b[1;32m✓ Tool Result:\x1b[0m"
                    };
                    println!("{badge} \x1b[1m{}\x1b[0m \x1b[2m({})\x1b[0m", tr.tool_name, tr.tool_call_id);
                    let preview = if tr.content.len() > 300 {
                        format!("{}... (truncated)", &tr.content[..300])
                    } else {
                        tr.content
                    };
                    for l in preview.lines() {
                        println!("\x1b[2m  {l}\x1b[0m");
                    }
                    println!();
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
