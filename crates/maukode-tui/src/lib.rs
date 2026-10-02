pub mod app;
pub mod event;
pub mod ui;

use std::io::stdout;
use std::path::PathBuf;
use std::time::Duration;
use anyhow::Result;
use crossterm::{
    event::{KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use maukode_core::{ProviderConfig, ProviderKind};
use maukode_protocol::SessionId;
use ratatui::{backend::CrosstermBackend, Terminal};
use crate::app::{ActiveModal, App, ChatRole};
use crate::event::{AppEvent, EventHandler};

pub async fn run_tui(
    cwd: PathBuf,
    session_id: Option<SessionId>,
    continue_latest: bool,
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
    let mut app = App::new(cwd, event_tx, session_id, continue_latest)?;

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
    while !app.should_quit {
        terminal
            .draw(|f| ui::draw(f, app))
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if let Some(event) = events.next().await {
            match event {
                AppEvent::Input(key) => {
                    handle_key(app, key);
                }
                AppEvent::Mouse(mouse) => {
                    match mouse.kind {
                        crossterm::event::MouseEventKind::ScrollUp => {
                            app.scroll_up(3);
                        }
                        crossterm::event::MouseEventKind::ScrollDown => {
                            app.scroll_down(3);
                        }
                        _ => {}
                    }
                }
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
            }
        }
    }
    Ok(())
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
                    }
                }
                KeyCode::Down => {
                    if total > 0 && *selected_idx + 1 < total {
                        *selected_idx += 1;
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
                        app.set_status(format!("Activated provider '{name}'"));
                        app.items.push(crate::app::ChatItem {
                            role: ChatRole::System,
                            content: format!("Switched to provider '{name}'. Active model: '{}'", app.config.active_model),
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
                *active_field = if *active_field == 0 { 3 } else { *active_field - 1 };
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
                0 => { name.pop(); }
                1 => { base_url.pop(); }
                2 => { api_key.pop(); }
                3 => { default_model.pop(); }
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
                    }
                }
                KeyCode::Down => {
                    if total > 0 && *selected_idx + 1 < total {
                        *selected_idx += 1;
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
                    app.set_status(format!("Model set to '{chosen}'"));
                    app.items.push(crate::app::ChatItem {
                        role: ChatRole::System,
                        content: format!("Model set to '{chosen}' for provider '{}'", app.config.active_provider),
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
                }
            }
            KeyCode::Down => {
                if !sessions.is_empty() && *selected_idx + 1 < sessions.len() {
                    *selected_idx += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(target) = sessions.get(*selected_idx) {
                    let target_id = target.id;
                    let loaded = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(async {
                            app.store.load_session(target_id).await.unwrap_or(None)
                        })
                    });
                    if let Some(loaded_session) = loaded {
                        let messages = loaded_session.messages.clone();
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
                                app.items.push(crate::app::ChatItem { role, content: text });
                            }
                        }
                        app.set_status(format!("Loaded session {}", target_id));
                    }
                }
                app.active_modal = ActiveModal::None;
            }
            _ => {}
        },
        ActiveModal::Help => {
            if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
                app.active_modal = ActiveModal::None;
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
