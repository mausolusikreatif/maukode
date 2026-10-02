pub mod app;
pub mod event;
pub mod theme;
pub mod ui;

use crate::app::{ActiveModal, App, ChatRole};
use crate::event::{AppEvent, EventHandler};
use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use maukode_core::{ProviderConfig, ProviderKind};
use maukode_protocol::SessionId;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;

pub async fn run_tui(
    cwd: PathBuf,
    session_id: Option<SessionId>,
    continue_latest: bool,
    server_client: Option<std::sync::Arc<maukode_server::ServerClient>>,
) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let (mut events, event_tx) = EventHandler::new(Duration::from_millis(50));
    let mut app = App::new(cwd, event_tx, session_id, continue_latest, server_client).await?;

    let res = run_loop(&mut terminal, &mut app, &mut events).await;

    // Restore terminal safely
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = res {
        eprintln!("TUI Error: {e}");
    }

    Ok(())
}

async fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    events: &mut EventHandler,
) -> Result<()> {
    // Initial draw
    terminal
        .draw(|f| ui::draw(f, app))
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    while !app.should_quit {
        if let Some(event) = events.next().await {
            handle_app_event(app, event);

            // Drain any pending events that arrived in the same batch
            // (e.g. streaming tokens, rapid typing, or tick queue)
            while let Some(buffered) = events.try_next() {
                handle_app_event(app, buffered);
                if app.should_quit {
                    break;
                }
            }

            terminal
                .draw(|f| ui::draw(f, app))
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        } else {
            break;
        }
    }
    Ok(())
}

fn handle_app_event(app: &mut App, event: AppEvent) {
    match event {
        AppEvent::Input(key) => {
            handle_key(app, key);
        }
        AppEvent::Mouse(mouse) => match mouse.kind {
            crossterm::event::MouseEventKind::ScrollUp => match &mut app.active_modal {
                ActiveModal::ModelSelector { selected_idx, .. } => {
                    *selected_idx = selected_idx.saturating_sub(1);
                }
                ActiveModal::ProviderSelector { selected_idx } => {
                    *selected_idx = selected_idx.saturating_sub(1);
                }
                ActiveModal::SessionSelector { selected_idx, .. } => {
                    *selected_idx = selected_idx.saturating_sub(1);
                }
                ActiveModal::DiffViewer { scroll, .. } => {
                    *scroll = scroll.saturating_sub(3);
                }
                ActiveModal::CodebaseMapViewer { scroll, .. } => {
                    *scroll = scroll.saturating_sub(3);
                }
                _ => app.scroll_up(3),
            },
            crossterm::event::MouseEventKind::ScrollDown => match &mut app.active_modal {
                ActiveModal::ModelSelector {
                    models,
                    selected_idx,
                    filter,
                    ..
                } => {
                    let total = models
                        .iter()
                        .filter(|m| {
                            filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase())
                        })
                        .count();
                    if total > 0 && *selected_idx + 1 < total {
                        *selected_idx += 1;
                    }
                }
                ActiveModal::ProviderSelector { selected_idx } => {
                    let total = app.config.providers.len();
                    if total > 0 && *selected_idx + 1 < total {
                        *selected_idx += 1;
                    }
                }
                ActiveModal::SessionSelector {
                    sessions,
                    selected_idx,
                } => {
                    if !sessions.is_empty() && *selected_idx + 1 < sessions.len() {
                        *selected_idx += 1;
                    }
                }
                ActiveModal::DiffViewer {
                    scroll, max_scroll, ..
                } => {
                    *scroll = (*scroll + 3).min(*max_scroll);
                }
                ActiveModal::CodebaseMapViewer {
                    scroll, max_scroll, ..
                } => {
                    *scroll = (*scroll + 3).min(*max_scroll);
                }
                _ => app.scroll_down(3),
            },
            _ => {}
        },
        AppEvent::Tick => {}
        AppEvent::LLM(llm_event) => {
            app.handle_llm_event(llm_event);
        }
        AppEvent::TurnFinished(result) => {
            app.handle_turn_finished(result);
        }
        AppEvent::ModelsFetched {
            provider_id,
            models,
        } => {
            if let ActiveModal::ModelSelector {
                models: ref mut m_list,
                ref mut is_loading,
                ..
            } = app.active_modal
            {
                *is_loading = false;
                match models {
                    Ok(fetched) => *m_list = fetched,
                    Err(err) => {
                        app.set_status(format!("Failed to fetch models for {provider_id}: {err}"));
                    }
                }
            }
        }
        AppEvent::DiffFetched(resp) => {
            let line_count = resp.diff_content.lines().count();
            app.active_modal = ActiveModal::DiffViewer {
                diff: resp.diff_content,
                stat: resp.stat_summary,
                scroll: 0,
                max_scroll: line_count.saturating_sub(10),
            };
            app.set_status("Diff viewer opened");
        }
        AppEvent::UndoCompleted(res) => match res {
            Ok(msg) => {
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: format!("Undo successful: {msg}"),
                });
                app.set_status(format!("Undo: {msg}"));
            }
            Err(e) => {
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: format!("Undo failed: {e}"),
                });
                app.set_status(format!("Undo failed: {e}"));
            }
        },
        AppEvent::CodebaseMapFetched(res) => match res {
            Ok(map) => {
                let line_count = map.lines().count();
                app.active_modal = ActiveModal::CodebaseMapViewer {
                    map,
                    scroll: 0,
                    max_scroll: line_count.saturating_sub(10),
                };
                app.set_status("Codebase map opened");
            }
            Err(e) => {
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: format!("Failed to generate codebase map: {e}"),
                });
                app.set_status(format!("Map failed: {e}"));
            }
        },
        AppEvent::InitProjectCompleted(res) => match res {
            Ok(resp) => {
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: format!("{}\nFile: {}", resp.message, resp.path),
                });
                app.set_status(&resp.message);
            }
            Err(e) => {
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: format!("Failed to initialize AGENTS.md: {e}"),
                });
                app.set_status(format!("Init failed: {e}"));
            }
        },
        AppEvent::McpStatusFetched(res) => match res {
            Ok(resp) => {
                let mut msg = String::from("=== Model Context Protocol (MCP) Servers ===\n");
                if resp.servers.is_empty() {
                    msg.push_str("No MCP servers configured.\nTo add MCP servers, configure 'mcpServers' in .maukode/mcp.json or ~/.config/maukode/config.json");
                } else {
                    for s in &resp.servers {
                        let icon = match s.status.as_str() {
                            "connected" => "●",
                            "error" => "▲",
                            _ => "○",
                        };
                        msg.push_str(&format!("{icon} {} ({})\n", s.name, s.status));
                        msg.push_str(&format!("  Command: {}\n", s.command));
                        if let Some(ref err) = s.error {
                            msg.push_str(&format!("  Error: {}\n", err));
                        }
                        if s.tools.is_empty() {
                            msg.push_str("  Tools: none\n");
                        } else {
                            msg.push_str(&format!("  Tools ({}):\n", s.tools.len()));
                            for t in &s.tools {
                                msg.push_str(&format!("    - {}: {}\n", t.name, t.description));
                            }
                        }
                    }
                }
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: msg.trim_end().to_string(),
                });
                app.set_status("MCP server status displayed");
            }
            Err(e) => {
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: format!("Failed to query MCP servers: {e}"),
                });
                app.set_status(format!("MCP query failed: {e}"));
            }
        },
        AppEvent::SessionCompacted(res) => match res {
            Ok(resp) => {
                let saved_tokens = resp.original_tokens.saturating_sub(resp.compacted_tokens);
                let saved_pct = if resp.original_tokens > 0 {
                    (saved_tokens as f32 / resp.original_tokens as f32) * 100.0
                } else {
                    0.0
                };
                let content = format!(
                    "✨ Conversation Compacted Successfully!\n\
                     • Messages: {} → {}\n\
                     • Estimated Tokens: ~{} → ~{} ({:.1}% saved)\n\n\
                     Summary:\n{}",
                    resp.original_messages,
                    resp.compacted_messages,
                    resp.original_tokens,
                    resp.compacted_tokens,
                    saved_pct,
                    resp.summary.trim()
                );
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content,
                });
                app.set_status(format!(
                    "Compacted: {} msgs -> {} msgs ({:.0}% saved)",
                    resp.original_messages, resp.compacted_messages, saved_pct
                ));
            }
            Err(e) => {
                app.items.push(crate::app::ChatItem {
                    role: ChatRole::System,
                    content: format!("Failed to compact session: {e}"),
                });
                app.set_status(format!("Compaction failed: {e}"));
            }
        },
    }
}

fn handle_key(app: &mut App, key: crossterm::event::KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return;
    }

    match &mut app.active_modal {
        ActiveModal::None => handle_normal_key(app, key),
        ActiveModal::ProviderSelector { selected_idx } => {
            let total = app.config.providers.len();
            match key.code {
                KeyCode::Esc => {
                    app.active_modal = ActiveModal::None;
                }
                KeyCode::Up => {
                    if *selected_idx > 0 {
                        *selected_idx -= 1;
                    } else if total > 0 {
                        *selected_idx = total - 1;
                    }
                }
                KeyCode::Down => {
                    if total > 0 {
                        if *selected_idx + 1 < total {
                            *selected_idx += 1;
                        } else {
                            *selected_idx = 0;
                        }
                    }
                }
                KeyCode::PageUp => {
                    *selected_idx = selected_idx.saturating_sub(5);
                }
                KeyCode::PageDown => {
                    if total > 0 {
                        *selected_idx = (*selected_idx + 5).min(total - 1);
                    }
                }
                KeyCode::Home => {
                    *selected_idx = 0;
                }
                KeyCode::End => {
                    if total > 0 {
                        *selected_idx = total - 1;
                    }
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    app.open_add_provider_modal();
                }
                KeyCode::Enter => {
                    if let Some(provider) = app.config.providers.get(*selected_idx) {
                        let id = provider.id.clone();
                        let name = provider.name.clone();
                        let _ = app.config.set_active_provider(&id);
                        if let Some(ref client) = app.server_client {
                            let client = client.clone();
                            let id_clone = id.clone();
                            tokio::spawn(async move {
                                let _ = client.set_active_provider(&id_clone).await;
                            });
                        }
                        app.set_status(format!("Activated provider '{name}'"));
                        app.items.push(crate::app::ChatItem {
                            role: ChatRole::System,
                            content: format!(
                                "Switched to provider '{name}'. Active model: '{}'",
                                app.config.active_model
                            ),
                        });
                    }
                    app.active_modal = ActiveModal::None;
                }
                _ => {}
            }
        }
        ActiveModal::AddProvider {
            name,
            base_url,
            api_key,
            default_model,
            active_field,
        } => match key.code {
            KeyCode::Esc => {
                app.active_modal = ActiveModal::None;
            }
            KeyCode::Tab => {
                *active_field = (*active_field + 1) % 4;
            }
            KeyCode::BackTab => {
                *active_field = if *active_field == 0 {
                    3
                } else {
                    *active_field - 1
                };
            }
            KeyCode::Enter => {
                let p_name = name.trim().to_string();
                let p_url = base_url.trim().to_string();
                let p_key = if api_key.trim().is_empty() {
                    None
                } else {
                    Some(api_key.trim().to_string())
                };
                let p_model = if default_model.trim().is_empty() {
                    None
                } else {
                    Some(default_model.trim().to_string())
                };

                if !p_name.is_empty() && !p_url.is_empty() {
                    let id = p_name.to_lowercase().replace(' ', "-");
                    let prov = ProviderConfig {
                        id: id.clone(),
                        name: p_name.clone(),
                        kind: ProviderKind::OpenAiCompatible,
                        base_url: p_url.clone(),
                        api_key: p_key,
                        default_model: p_model,
                    };
                    app.config.add_or_update_provider(prov);
                    let _ = app.config.set_active_provider(&id);
                    app.set_status(format!("Added provider '{p_name}' and activated"));
                    app.items.push(crate::app::ChatItem {
                        role: ChatRole::System,
                        content: format!("Added OpenAI-compatible provider '{p_name}' ({p_url}) and set as active."),
                    });
                }
                app.active_modal = ActiveModal::None;
            }
            KeyCode::Backspace => match *active_field {
                0 => {
                    name.pop();
                }
                1 => {
                    base_url.pop();
                }
                2 => {
                    api_key.pop();
                }
                3 => {
                    default_model.pop();
                }
                _ => {}
            },
            KeyCode::Char(c) => match *active_field {
                0 => name.push(c),
                1 => base_url.push(c),
                2 => api_key.push(c),
                3 => default_model.push(c),
                _ => {}
            },
            _ => {}
        },
        ActiveModal::ModelSelector {
            models,
            selected_idx,
            filter,
            ..
        } => {
            let filtered: Vec<String> = models
                .iter()
                .filter(|m| filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase()))
                .cloned()
                .collect();
            let total = filtered.len();

            match key.code {
                KeyCode::Esc => {
                    app.active_modal = ActiveModal::None;
                }
                KeyCode::Up => {
                    if *selected_idx > 0 {
                        *selected_idx -= 1;
                    } else if total > 0 {
                        *selected_idx = total - 1;
                    }
                }
                KeyCode::Down => {
                    if total > 0 {
                        if *selected_idx + 1 < total {
                            *selected_idx += 1;
                        } else {
                            *selected_idx = 0;
                        }
                    }
                }
                KeyCode::PageUp => {
                    *selected_idx = selected_idx.saturating_sub(5);
                }
                KeyCode::PageDown => {
                    if total > 0 {
                        *selected_idx = (*selected_idx + 5).min(total - 1);
                    }
                }
                KeyCode::Home => {
                    *selected_idx = 0;
                }
                KeyCode::End => {
                    if total > 0 {
                        *selected_idx = total - 1;
                    }
                }
                KeyCode::Tab => {
                    if total > 0 {
                        if *selected_idx + 1 < total {
                            *selected_idx += 1;
                        } else {
                            *selected_idx = 0;
                        }
                    }
                }
                KeyCode::BackTab => {
                    if *selected_idx > 0 {
                        *selected_idx -= 1;
                    } else if total > 0 {
                        *selected_idx = total - 1;
                    }
                }
                KeyCode::Backspace => {
                    filter.pop();
                    *selected_idx = 0;
                }
                KeyCode::Enter => {
                    let chosen = if let Some(m) = filtered.get(*selected_idx) {
                        m.clone()
                    } else if !filter.trim().is_empty() {
                        filter.trim().to_string()
                    } else {
                        app.config.active_model.clone()
                    };

                    app.config.set_active_model(&chosen);
                    if let Some(ref client) = app.server_client {
                        let client = client.clone();
                        let chosen_clone = chosen.clone();
                        tokio::spawn(async move {
                            let _ = client.set_active_model(&chosen_clone).await;
                        });
                    }
                    app.set_status(format!("Model set to '{chosen}'"));
                    app.items.push(crate::app::ChatItem {
                        role: ChatRole::System,
                        content: format!(
                            "Model set to '{chosen}' for provider '{}'",
                            app.config.active_provider
                        ),
                    });
                    app.active_modal = ActiveModal::None;
                }
                KeyCode::Char(c) => {
                    filter.push(c);
                    *selected_idx = 0;
                }
                _ => {}
            }
        }
        ActiveModal::SessionSelector {
            sessions,
            selected_idx,
        } => match key.code {
            KeyCode::Esc => {
                app.active_modal = ActiveModal::None;
            }
            KeyCode::Up => {
                if *selected_idx > 0 {
                    *selected_idx -= 1;
                } else if !sessions.is_empty() {
                    *selected_idx = sessions.len() - 1;
                }
            }
            KeyCode::Down => {
                if !sessions.is_empty() {
                    if *selected_idx + 1 < sessions.len() {
                        *selected_idx += 1;
                    } else {
                        *selected_idx = 0;
                    }
                }
            }
            KeyCode::PageUp => {
                *selected_idx = selected_idx.saturating_sub(5);
            }
            KeyCode::PageDown => {
                if !sessions.is_empty() {
                    *selected_idx = (*selected_idx + 5).min(sessions.len() - 1);
                }
            }
            KeyCode::Home => {
                *selected_idx = 0;
            }
            KeyCode::End => {
                if !sessions.is_empty() {
                    *selected_idx = sessions.len() - 1;
                }
            }
            KeyCode::Enter => {
                if let Some(target) = sessions.get(*selected_idx) {
                    let target_id = target.id;
                    let loaded = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            if let Some(ref client) = app.server_client {
                                if let Ok(detail) = client.get_session(target_id).await {
                                    let mut s = maukode_core::Session::new(&detail.directory);
                                    s.id = detail.id;
                                    s.messages = detail.messages;
                                    s.state = detail.state;
                                    return Some(s);
                                }
                            }
                            app.store.load_session(target_id).await.unwrap_or(None)
                        })
                    });
                    if let Some(loaded_session) = loaded {
                        let messages = loaded_session.messages.clone();
                        app.cwd = loaded_session.directory.clone();
                        app.tools =
                            std::sync::Arc::new(maukode_tools::create_default_registry(&app.cwd));
                        app.context =
                            std::sync::Arc::new(maukode_core::ContextManager::new(&app.cwd));
                        app.session = std::sync::Arc::new(tokio::sync::RwLock::new(loaded_session));
                        app.items.clear();
                        for msg in &messages {
                            let text = msg.text_content();
                            if !text.is_empty() {
                                let role = match msg.role {
                                    maukode_protocol::Role::User => ChatRole::User,
                                    maukode_protocol::Role::Assistant => ChatRole::Assistant,
                                    maukode_protocol::Role::System => ChatRole::System,
                                };
                                app.items.push(crate::app::ChatItem {
                                    role,
                                    content: text,
                                });
                            }
                        }
                        app.set_status(format!(
                            "Loaded session {} ({})",
                            target_id,
                            app.cwd.display()
                        ));
                    }
                }
                app.active_modal = ActiveModal::None;
            }
            _ => {}
        },
        ActiveModal::DiffViewer {
            scroll, max_scroll, ..
        } => match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.active_modal = ActiveModal::None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                *scroll = scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                *scroll = (*scroll + 1).min(*max_scroll);
            }
            KeyCode::PageUp => {
                *scroll = scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                *scroll = (*scroll + 10).min(*max_scroll);
            }
            KeyCode::Home => {
                *scroll = 0;
            }
            KeyCode::End => {
                *scroll = *max_scroll;
            }
            KeyCode::Char('u') | KeyCode::Char('U') => {
                app.active_modal = ActiveModal::None;
                app.undo_turn(None);
            }
            _ => {}
        },
        ActiveModal::CodebaseMapViewer {
            scroll, max_scroll, ..
        } => match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.active_modal = ActiveModal::None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                *scroll = scroll.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                *scroll = (*scroll + 1).min(*max_scroll);
            }
            KeyCode::PageUp => {
                *scroll = scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                *scroll = (*scroll + 10).min(*max_scroll);
            }
            KeyCode::Home => {
                *scroll = 0;
            }
            KeyCode::End => {
                *scroll = *max_scroll;
            }
            _ => {}
        },
        ActiveModal::Help => {
            if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
                app.active_modal = ActiveModal::None;
            }
        }
        ActiveModal::PermissionConfirmation { request } => {
            let req = request.clone();
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    app.respond_permission(req, maukode_protocol::PermissionDecision::AllowOnce);
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    app.respond_permission(req, maukode_protocol::PermissionDecision::AllowSession);
                }
                KeyCode::Char('n')
                | KeyCode::Char('N')
                | KeyCode::Char('q')
                | KeyCode::Char('Q')
                | KeyCode::Esc => {
                    app.respond_permission(req, maukode_protocol::PermissionDecision::Deny);
                }
                _ => {}
            }
        }
    }
}

fn handle_normal_key(app: &mut App, key: crossterm::event::KeyEvent) {
    match key.code {
        KeyCode::Enter => {
            app.submit_prompt();
        }
        KeyCode::Backspace => {
            if app.cursor_pos > 0 && app.cursor_pos <= app.input.len() {
                app.input.remove(app.cursor_pos - 1);
                app.cursor_pos -= 1;
            }
        }
        KeyCode::Delete => {
            if app.cursor_pos < app.input.len() {
                app.input.remove(app.cursor_pos);
            }
        }
        KeyCode::Left => {
            if app.cursor_pos > 0 {
                app.cursor_pos -= 1;
            }
        }
        KeyCode::Right => {
            if app.cursor_pos < app.input.len() {
                app.cursor_pos += 1;
            }
        }
        KeyCode::Home => {
            if app.input.is_empty() {
                app.scroll_to_top();
            } else {
                app.cursor_pos = 0;
            }
        }
        KeyCode::End => {
            if app.input.is_empty() {
                app.scroll_to_bottom();
            } else {
                app.cursor_pos = app.input.len();
            }
        }
        KeyCode::Up => {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT)
                || key.modifiers.contains(KeyModifiers::SHIFT)
                || app.input.is_empty()
            {
                app.scroll_up(2);
            }
        }
        KeyCode::Down => {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                || key.modifiers.contains(KeyModifiers::ALT)
                || key.modifiers.contains(KeyModifiers::SHIFT)
                || app.input.is_empty()
            {
                app.scroll_down(2);
            }
        }
        KeyCode::PageUp => {
            app.scroll_up(10);
        }
        KeyCode::PageDown => {
            app.scroll_down(10);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_up(10);
        }
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_down(10);
        }
        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_up(2);
        }
        KeyCode::Char('n') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.scroll_down(2);
        }
        KeyCode::Char(c) => {
            app.input.insert(app.cursor_pos, c);
            app.cursor_pos += 1;
        }
        KeyCode::Esc => {
            if !app.input.is_empty() {
                app.input.clear();
                app.cursor_pos = 0;
            } else {
                // If input is already empty, reset scroll to bottom
                app.scroll_to_bottom();
            }
        }
        _ => {}
    }
}
