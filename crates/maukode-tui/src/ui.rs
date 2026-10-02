use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap,
    },
    Frame,
};
use crate::app::{ActiveModal, App, ChatRole};

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Messages viewport
            Constraint::Length(3), // Input box
            Constraint::Length(1), // Footer / Status bar
        ])
        .split(size);

    draw_header(f, app, chunks[0]);
    draw_messages(f, app, chunks[1]);
    draw_input(f, app, chunks[2]);
    draw_footer(f, app, chunks[3]);

    // Draw active modal overlay if any
    match &app.active_modal {
        ActiveModal::None => {}
        ActiveModal::ProviderSelector { selected_idx } => {
            draw_provider_modal(f, app, *selected_idx);
        }
        ActiveModal::AddProvider {
            name,
            base_url,
            api_key,
            default_model,
            active_field,
        } => {
            draw_add_provider_modal(
                f,
                name,
                base_url,
                api_key,
                default_model,
                *active_field,
            );
        }
        ActiveModal::ModelSelector {
            models,
            selected_idx,
            is_loading,
            filter,
        } => {
            draw_model_modal(f, app, models, *selected_idx, *is_loading, filter);
        }
        ActiveModal::SessionSelector {
            sessions,
            selected_idx,
        } => {
            draw_sessions_modal(f, sessions, *selected_idx);
        }
        ActiveModal::Help => {
            draw_help_modal(f);
        }
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let active_prov = &app.config.active_provider;
    let active_model = &app.config.active_model;
    let session_guard = app.session.try_read();
    let session_id_str = match session_guard {
        Ok(s) => s.id.to_string()[..8].to_string(),
        Err(_) => "....".to_string(),
    };

    let title_spans = vec![
        Span::styled(" 🤖 MauKode ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(" │ Provider: "),
        Span::styled(active_prov, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" │ Model: "),
        Span::styled(active_model, Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw(" │ Session: "),
        Span::styled(session_id_str, Style::default().fg(Color::Blue)),
        Span::raw(" │ CWD: "),
        Span::styled(app.cwd.file_name().and_then(|n| n.to_str()).unwrap_or("."), Style::default().fg(Color::DarkGray)),
    ];

    let header_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));

    let header = Paragraph::new(Line::from(title_spans))
        .block(header_block)
        .alignment(Alignment::Left);

    f.render_widget(header, area);
}

fn draw_messages(f: &mut Frame, app: &mut App, area: Rect) {
    let mut lines = Vec::new();

    for item in &app.items {
        match &item.role {
            ChatRole::User => {
                lines.push(Line::from(vec![
                    Span::styled("❯ You: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled(&item.content, Style::default().fg(Color::White)),
                ]));
                lines.push(Line::from(""));
            }
            ChatRole::Assistant => {
                lines.push(Line::from(vec![
                    Span::styled("● MauKode: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                ]));
                for l in item.content.lines() {
                    lines.push(Line::from(format!("  {l}")));
                }
                lines.push(Line::from(""));
            }
            ChatRole::Reasoning => {
                lines.push(Line::from(vec![
                    Span::styled("💭 Thinking: ", Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                ]));
                for l in item.content.lines() {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {l}"), Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)),
                    ]));
                }
                lines.push(Line::from(""));
            }
            ChatRole::ToolCall { name, id, args } => {
                lines.push(Line::from(vec![
                    Span::styled("⚡ Tool Call: ", Style::default().fg(Color::LightMagenta).add_modifier(Modifier::BOLD)),
                    Span::styled(name, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    Span::styled(format!(" ({id})"), Style::default().fg(Color::DarkGray)),
                ]));
                if !args.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  Args: {args}"), Style::default().fg(Color::DarkGray)),
                    ]));
                }
                lines.push(Line::from(""));
            }
            ChatRole::ToolResult { name, is_error, .. } => {
                let badge = if *is_error {
                    Span::styled("✗ Tool Error: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                } else {
                    Span::styled("✓ Tool Result: ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                };
                lines.push(Line::from(vec![
                    badge,
                    Span::styled(name, Style::default().fg(Color::White)),
                ]));
                let content_preview = if item.content.len() > 300 {
                    format!("{}... (truncated)", &item.content[..300])
                } else {
                    item.content.clone()
                };
                for l in content_preview.lines() {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {l}"), Style::default().fg(Color::DarkGray)),
                    ]));
                }
                lines.push(Line::from(""));
            }
            ChatRole::System => {
                lines.push(Line::from(vec![
                    Span::styled("ℹ ", Style::default().fg(Color::Yellow)),
                    Span::styled(&item.content, Style::default().fg(Color::DarkGray)),
                ]));
                lines.push(Line::from(""));
            }
        }
    }

    let inner_width = area.width.saturating_sub(4).max(1) as usize;
    let mut total_visual_lines = 0;
    for line in &lines {
        let line_len: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
        let wrapped = (line_len + inner_width - 1) / inner_width;
        total_visual_lines += wrapped.max(1);
    }

    let viewport_height = area.height.saturating_sub(2) as usize;
    app.max_scroll = total_visual_lines.saturating_sub(viewport_height);

    if app.auto_scroll {
        app.scroll = app.max_scroll;
    } else {
        app.scroll = app.scroll.min(app.max_scroll);
    }

    let title = if app.auto_scroll {
        " Conversation (Live) ".to_string()
    } else {
        format!(
            " Conversation [Scrolled {}/{} - Press End/PageDown to resume] ",
            app.scroll, app.max_scroll
        )
    };

    let title_style = if app.auto_scroll {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(title, title_style));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.scroll as u16, 0));

    f.render_widget(paragraph, area);

    // Render scrollbar on the right border
    if app.max_scroll > 0 {
        use ratatui::widgets::{Scrollbar, ScrollbarOrientation, ScrollbarState};
        let mut scrollbar_state = ScrollbarState::new(app.max_scroll).position(app.scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some("│"))
            .thumb_symbol("█");

        f.render_stateful_widget(
            scrollbar,
            area.inner(ratatui::layout::Margin { vertical: 1, horizontal: 0 }),
            &mut scrollbar_state,
        );
    }
}

fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let is_busy = app.is_running;
    let border_color = if is_busy {
        Color::Yellow
    } else {
        Color::Green
    };

    let title = if is_busy {
        " MauKode is thinking... (press Esc to cancel) "
    } else {
        " Prompt (type /provider, /model, /new, /help) "
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title);

    let input_line = Line::from(vec![
        Span::styled("❯ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(&app.input),
    ]);

    let paragraph = Paragraph::new(input_line).block(block);
    f.render_widget(paragraph, area);

    // Render blinking cursor if active
    if !is_busy {
        f.set_cursor_position((
            area.x + 3 + app.cursor_pos as u16,
            area.y + 1,
        ));
    }
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let mut spans = vec![
        Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
        Span::raw("Send  "),
        Span::styled("[/provider] ", Style::default().fg(Color::Yellow)),
        Span::raw("Switch/Add  "),
        Span::styled("[/model] ", Style::default().fg(Color::Magenta)),
        Span::raw("Change Model  "),
        Span::styled("[/new] ", Style::default().fg(Color::Green)),
        Span::raw("New Session  "),
        Span::styled("[/help] ", Style::default().fg(Color::White)),
        Span::raw("Help  "),
        Span::styled("[Scroll: ", Style::default().fg(Color::Blue)),
        Span::raw("Wheel/PageUp/Dn]  "),
        Span::styled("[Ctrl+C] ", Style::default().fg(Color::Red)),
        Span::raw("Quit"),
    ];

    if let Some(ref usage) = app.token_usage {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(
            format!("In: {} | Out: {}", usage.input_tokens, usage.output_tokens),
            Style::default().fg(Color::DarkGray),
        ));
    }

    if let Some((ref status, _)) = app.status_message {
        spans.push(Span::raw(" │ "));
        spans.push(Span::styled(status, Style::default().fg(Color::Yellow)));
    }

    let footer = Paragraph::new(Line::from(spans)).alignment(Alignment::Left);
    f.render_widget(footer, area);
}

// -------------------------------------------------------------
// Modals
// -------------------------------------------------------------

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn draw_provider_modal(f: &mut Frame, app: &App, selected_idx: usize) {
    let area = centered_rect(65, 60, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Yellow))
        .title(" Select AI Provider (Press 'a' to add OpenAI-compatible provider) ");

    let items: Vec<ListItem> = app
        .config
        .providers
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let is_active = p.id == app.config.active_provider;
            let is_selected = idx == selected_idx;

            let prefix = if is_selected { "▶ " } else { "  " };
            let active_badge = if is_active {
                Span::styled(" [ACTIVE]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            };

            let style = if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&p.name, style),
                Span::styled(format!(" ({})", p.base_url), Style::default().fg(Color::DarkGray)),
                active_badge,
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_widget(list, area);
}

fn draw_add_provider_modal(
    f: &mut Frame,
    name: &str,
    base_url: &str,
    api_key: &str,
    default_model: &str,
    active_field: usize,
) {
    let area = centered_rect(65, 55, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Add OpenAI-Compatible Provider (Tab to switch fields, Enter to save) ");

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Name
            Constraint::Length(3), // Base URL
            Constraint::Length(3), // API Key
            Constraint::Length(3), // Default Model
            Constraint::Length(2), // Hint
        ])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    let fields = [
        ("Provider Name (e.g. DeepSeek, Local vLLM, Ollama):", name),
        ("Base URL (e.g. http://localhost:11434 or https://api.deepseek.com):", base_url),
        ("API Key (optional for local models):", api_key),
        ("Default Model (e.g. deepseek-chat, qwen2.5-coder):", default_model),
    ];

    for (idx, (label, val)) in fields.iter().enumerate() {
        let is_active = idx == active_field;
        let border_color = if is_active { Color::Green } else { Color::DarkGray };

        let field_block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(*label);

        let p = Paragraph::new(*val).block(field_block);
        f.render_widget(p, inner[idx]);
    }

    let hint = Paragraph::new(Line::from(vec![
        Span::styled("[Tab] ", Style::default().fg(Color::Yellow)),
        Span::raw("Next Field  "),
        Span::styled("[Enter] ", Style::default().fg(Color::Green)),
        Span::raw("Save & Activate  "),
        Span::styled("[Esc] ", Style::default().fg(Color::Red)),
        Span::raw("Cancel"),
    ])).alignment(Alignment::Center);

    f.render_widget(hint, inner[4]);
}

fn draw_model_modal(
    f: &mut Frame,
    app: &App,
    models: &[String],
    selected_idx: usize,
    is_loading: bool,
    filter: &str,
) {
    let area = centered_rect(65, 70, f.area());
    f.render_widget(Clear, area);

    let title = format!(" Select Model for Provider '{}' ", app.config.active_provider);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Magenta))
        .title(title);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Filter box
            Constraint::Min(5),    // Model list
        ])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    // Filter box
    let filter_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Type to filter or enter custom model: ");
    let filter_p = Paragraph::new(filter).block(filter_block);
    f.render_widget(filter_p, inner[0]);

    if is_loading {
        let loading_p = Paragraph::new("\n  ⏳ Fetching models from provider API...")
            .alignment(Alignment::Center);
        f.render_widget(loading_p, inner[1]);
        return;
    }

    let filtered_models: Vec<&String> = models
        .iter()
        .filter(|m| filter.is_empty() || m.to_lowercase().contains(&filter.to_lowercase()))
        .collect();

    let items: Vec<ListItem> = filtered_models
        .iter()
        .enumerate()
        .map(|(idx, m)| {
            let is_active = *m == &app.config.active_model;
            let is_selected = idx == selected_idx;

            let prefix = if is_selected { "▶ " } else { "  " };
            let active_badge = if is_active {
                Span::styled(" [ACTIVE]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
            } else {
                Span::raw("")
            };

            let style = if is_selected {
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(*m, style),
                active_badge,
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::NONE))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_widget(list, inner[1]);
}

fn draw_sessions_modal(f: &mut Frame, sessions: &[maukode_core::SessionSummary], selected_idx: usize) {
    let area = centered_rect(70, 70, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Blue))
        .title(" Resume Session History (Enter to load, Esc to close) ");

    let items: Vec<ListItem> = sessions
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let is_selected = idx == selected_idx;
            let prefix = if is_selected { "▶ " } else { "  " };
            let style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let time_str = s.updated_at.format("%Y-%m-%d %H:%M").to_string();

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&s.title, style),
                Span::styled(format!(" [{}]", s.model), Style::default().fg(Color::DarkGray)),
                Span::styled(format!(" ({time_str}, {} msgs)", s.message_count), Style::default().fg(Color::Blue)),
            ]))
        })
        .collect();

    let list = List::new(items).block(block);
    f.render_widget(list, area);
}

fn draw_help_modal(f: &mut Frame) {
    let area = centered_rect(60, 50, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Green))
        .title(" MauKode Help & Slash Commands ");

    let help_text = vec![
        Line::from(vec![
            Span::styled("/provider       ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw("Open provider menu to switch or add OpenAI-compatible providers"),
        ]),
        Line::from(vec![
            Span::styled("/model          ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::raw("Open model menu to fetch & choose model from current provider"),
        ]),
        Line::from(vec![
            Span::styled("/new            ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("Start a new clean session"),
        ]),
        Line::from(vec![
            Span::styled("/sessions       ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            Span::raw("View and resume previous saved sessions from SQLite"),
        ]),
        Line::from(vec![
            Span::styled("/clear          ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw("Clear the current conversation view"),
        ]),
        Line::from(vec![
            Span::styled("/exit, /quit    ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw("Exit MauKode"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Keys: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("PageUp/PageDown to scroll, Esc to dismiss dialogs, Ctrl+C to quit."),
        ]),
    ];

    let p = Paragraph::new(help_text).block(block);
    f.render_widget(p, area);
}
