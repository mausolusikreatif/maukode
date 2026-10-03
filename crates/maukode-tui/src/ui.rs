use crate::app::{ActiveModal, App, ChatRole};
use crate::theme::{self, *};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Wrap,
    },
    Frame,
};
use unicode_width::UnicodeWidthChar;

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    // Grow the prompt up to six rows while retaining space for chat and hints.
    let desired_input_rows = wrapped_input(&app.input, app.cursor_pos, size.width)
        .0
        .len()
        .min(u16::MAX as usize) as u16;
    let input_rows = desired_input_rows
        .min(6)
        .min(size.height.saturating_sub(6))
        .max(1);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),              // Minimalist telemetry status bar
            Constraint::Min(4),                 // Chat viewport
            Constraint::Length(input_rows + 2), // Divider, wrapped prompt, hints
        ])
        .split(size);

    draw_header(f, app, chunks[0]);
    draw_messages(
        f,
        &app.items,
        &mut app.render_cache,
        &mut app.scroll,
        &mut app.max_scroll,
        app.auto_scroll,
        chunks[1],
    );
    draw_input(
        f,
        &app.input,
        app.cursor_pos,
        app.is_running,
        app.spinner(),
        chunks[2],
    );

    // Active modal overlays
    let mut permission_max_scroll = None;
    match &app.active_modal {
        ActiveModal::None => {}
        ActiveModal::PrivateResult {
            tool_name,
            content,
            scroll,
        } => {
            draw_private_result_modal(f, tool_name, content, *scroll);
        }
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
            draw_add_provider_modal(f, name, base_url, api_key, default_model, *active_field);
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
        ActiveModal::DiffViewer {
            diff, stat, scroll, ..
        } => {
            draw_diff_modal(f, diff, stat, *scroll);
        }
        ActiveModal::CodebaseMapViewer { map, scroll, .. } => {
            draw_codebase_map_modal(f, map, *scroll);
        }
        ActiveModal::Help => {
            draw_help_modal(f);
        }
        ActiveModal::PermissionConfirmation { request, scroll } => {
            permission_max_scroll = Some(draw_permission_modal(f, request, *scroll));
        }
        ActiveModal::AuditViewer { content, scroll } => {
            draw_audit_modal(f, content, *scroll);
        }
    }
    if let (Some(max_scroll), ActiveModal::PermissionConfirmation { scroll, .. }) =
        (permission_max_scroll, &mut app.active_modal)
    {
        *scroll = (*scroll).min(max_scroll);
    }
}

// -------------------------------------------------------------
// Header: Sleek 1-line Telemetry
// -------------------------------------------------------------

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let active_prov = &app.config.active_provider;
    let active_model = &app.config.active_model;
    let session_guard = app.session.try_read();
    let session_id_str = match session_guard {
        Ok(s) => s.id.to_string()[..8].to_string(),
        Err(_) => "....".to_string(),
    };

    let (mode_badge, mode_color) = if let Some(ref client) = app.server_client {
        if client.base_url().contains("127.0.0.1") || client.base_url().contains("localhost") {
            ("● daemon:4096", COLOR_MINT)
        } else {
            ("● remote:vps", COLOR_CYAN)
        }
    } else {
        ("○ standalone", COLOR_MUTED)
    };

    let cwd_name = app.cwd.file_name().and_then(|n| n.to_str()).unwrap_or(".");

    let mut left_spans = vec![
        Span::styled(" maukode ", theme::brand_style()),
        Span::styled(mode_badge, Style::default().fg(mode_color)),
        Span::styled(" │ ", theme::border_style()),
        Span::styled(format!("📁 {cwd_name}"), theme::text_style()),
        Span::styled(" │ ", theme::border_style()),
        Span::styled(
            format!("{active_prov}/{active_model}"),
            theme::muted_style(),
        ),
        Span::styled(" │ ", theme::border_style()),
        Span::styled(
            format!("#{session_id_str}"),
            Style::default().fg(COLOR_BLUE),
        ),
    ];

    if let Some(ref usage) = app.token_usage {
        left_spans.push(Span::styled(" │ ", theme::border_style()));
        left_spans.push(Span::styled(
            format!("{} tok", usage.input_tokens + usage.output_tokens),
            theme::dim_style(),
        ));
    }

    if let Some((ref status, _)) = app.status_message {
        left_spans.push(Span::styled(" │ ", theme::border_style()));
        left_spans.push(Span::styled(status, Style::default().fg(COLOR_AMBER)));
    }

    let header_line = Paragraph::new(Line::from(left_spans)).alignment(Alignment::Left);
    f.render_widget(header_line, area);
}

// -------------------------------------------------------------
// Messages Viewport: Clean Typography, Cards, Diff Highlighting
// -------------------------------------------------------------

/// Build the styled lines for a single chat item. Always owned/`'static` so callers (the
/// per-item render cache in particular) can keep the result past the lifetime of `item`.
pub fn build_item_lines(item: &crate::app::ChatItem) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    match &item.role {
        ChatRole::User => {
            lines.push(Line::from(vec![Span::styled(
                "❯ You",
                theme::brand_style(),
            )]));
            for l in item.content.lines() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(l.to_string(), theme::text_style()),
                ]));
            }
            lines.push(Line::from(""));
        }
        ChatRole::Assistant => {
            lines.push(Line::from(vec![Span::styled(
                "● MauKode",
                Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
            )]));

            lines.extend(crate::markdown::render(&item.content));
            lines.push(Line::from(""));
        }
        ChatRole::Reasoning => {
            lines.push(Line::from(vec![Span::styled(
                "💭 Thinking",
                theme::thinking_style(),
            )]));
            for l in item.content.lines() {
                lines.push(Line::from(vec![
                    Span::styled("  │ ", theme::border_style()),
                    Span::styled(
                        l.to_string(),
                        Style::default()
                            .fg(COLOR_MUTED)
                            .add_modifier(Modifier::ITALIC),
                    ),
                ]));
            }
            lines.push(Line::from(""));
        }
        ChatRole::ToolCall { name, id, args } => {
            lines.push(Line::from(vec![
                Span::styled("╭─ ⚡ ", theme::tool_style()),
                Span::styled(
                    name.clone(),
                    Style::default().fg(COLOR_FG).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" ({id})"), theme::dim_style()),
            ]));
            if !args.is_empty() {
                lines.push(Line::from(vec![
                    Span::styled("│  ", theme::tool_style()),
                    Span::styled(format!("Args: {args}"), theme::muted_style()),
                ]));
            }
        }
        ChatRole::ToolResult { name, is_error, .. } => {
            let (prefix, badge_style) = if *is_error {
                ("╰─ ✗ Error: ", theme::error_style())
            } else {
                ("╰─ ✓ Result: ", theme::success_style())
            };

            lines.push(Line::from(vec![
                Span::styled(prefix, badge_style),
                Span::styled(name.clone(), theme::text_style()),
            ]));

            let (content_preview, is_truncated) = if item.content.len() > 600 {
                let end = item.content.floor_char_boundary(600);
                (&item.content[..end], true)
            } else {
                (item.content.as_str(), false)
            };

            for l in content_preview.lines() {
                let style = if l.starts_with('+') {
                    theme::diff_add_style()
                } else if l.starts_with('-') {
                    theme::diff_del_style()
                } else if l.starts_with("@@") {
                    theme::diff_hunk_style()
                } else {
                    theme::dim_style()
                };
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled(l.to_string(), style),
                ]));
            }
            if is_truncated {
                lines.push(Line::from(vec![
                    Span::raw("   "),
                    Span::styled("... (preview truncated)", theme::dim_style()),
                ]));
            }
            lines.push(Line::from(""));
        }
        ChatRole::System => {
            lines.push(Line::from(vec![
                Span::styled("ℹ ", Style::default().fg(COLOR_AMBER)),
                Span::styled(item.content.clone(), theme::muted_style()),
            ]));
            lines.push(Line::from(""));
        }
    }

    lines
}

/// Build the full styled line buffer for a conversation by concatenating each item's lines.
/// Pure and Frame-free so it can be measured directly in benchmarks
/// (`benches/render_messages.rs`) without constructing an `App`. Prefer
/// [`crate::render_cache::MessageRenderCache`] on the hot draw path — this recomputes
/// everything unconditionally and is O(total conversation size) every call.
pub fn build_message_lines(items: &[crate::app::ChatItem]) -> Vec<Line<'static>> {
    items.iter().flat_map(build_item_lines).collect()
}

fn draw_messages(
    f: &mut Frame,
    items: &[crate::app::ChatItem],
    render_cache: &mut crate::render_cache::MessageRenderCache,
    scroll: &mut usize,
    max_scroll: &mut usize,
    auto_scroll: bool,
    area: Rect,
) {
    let viewport_height = area.height.saturating_sub(1) as usize;
    let content_width = area.width.max(1);

    // Only items whose content actually changed since the last frame get rebuilt/rewrapped;
    // everything else is reused from the cache (see render_cache.rs for why this matters).
    render_cache.sync(items, content_width);
    *max_scroll = render_cache.total_height().saturating_sub(viewport_height);

    if auto_scroll {
        *scroll = *max_scroll;
    } else {
        *scroll = (*scroll).min(*max_scroll);
    }

    let title = if auto_scroll {
        " Live Session ".to_string()
    } else {
        format!(
            " Scrolled [{}/{}] ── Press End to resume ",
            *scroll, *max_scroll
        )
    };

    let title_style = if auto_scroll {
        theme::dim_style()
    } else {
        Style::default()
            .fg(COLOR_AMBER)
            .add_modifier(Modifier::BOLD)
    };

    let block = Block::default()
        .borders(Borders::TOP)
        .border_type(BorderType::Plain)
        .border_style(theme::border_style())
        .title(Span::styled(title, title_style));

    // Only the items overlapping the visible row range are concatenated, so Paragraph's wrap
    // pass below is bounded by viewport size, not conversation length.
    let (visible_lines, local_scroll) = render_cache.visible_lines(*scroll, viewport_height);
    let paragraph = Paragraph::new(visible_lines)
        .wrap(Wrap { trim: false })
        .block(block)
        .scroll((local_scroll, 0));

    f.render_widget(paragraph, area);

    // Render subtle scrollbar
    if *max_scroll > 0 {
        let mut scrollbar_state = ScrollbarState::new(*max_scroll).position(*scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme::dim_style());

        f.render_stateful_widget(
            scrollbar,
            area.inner(ratatui::layout::Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );
    }
}

// -------------------------------------------------------------
// Input & Contextual Hint Bar
// -------------------------------------------------------------

fn wrapped_input(input: &str, cursor_pos: usize, width: u16) -> (Vec<String>, usize, u16) {
    let content_width = width.saturating_sub(2).max(1) as usize;
    let chars: Vec<char> = input.chars().collect();
    let cursor_pos = cursor_pos.min(chars.len());
    let mut rows = vec![String::new()];
    let mut column = 0;
    let mut cursor = (0, 2);

    for (index, ch) in chars.iter().enumerate() {
        let char_width = UnicodeWidthChar::width(*ch).unwrap_or(0);
        if ch.is_whitespace() && column >= content_width {
            rows.push(String::new());
            column = 0;
        }
        if rows.len() > 1 && column == 0 && ch.is_whitespace() {
            if index == cursor_pos {
                cursor = (rows.len() - 1, 2);
            }
            continue;
        }
        if !ch.is_whitespace() && (index == 0 || chars[index - 1].is_whitespace()) {
            let word_width: usize = chars[index..]
                .iter()
                .take_while(|next| !next.is_whitespace())
                .map(|next| UnicodeWidthChar::width(*next).unwrap_or(0))
                .sum();
            if column > 0 && word_width <= content_width && column + word_width > content_width {
                rows.push(String::new());
                column = 0;
            }
        }
        if column > 0 && column + char_width > content_width {
            rows.push(String::new());
            column = 0;
        }
        if index == cursor_pos {
            cursor = (rows.len() - 1, (column + 2) as u16);
        }
        if let Some(row) = rows.last_mut() {
            row.push(*ch);
        }
        column += char_width;
    }
    if cursor_pos == chars.len() {
        if column >= content_width {
            rows.push(String::new());
            column = 0;
        }
        cursor = (rows.len() - 1, (column + 2) as u16);
    }
    (rows, cursor.0, cursor.1.min(width.saturating_sub(1)))
}

fn draw_input(
    f: &mut Frame,
    input: &str,
    cursor_pos: usize,
    is_busy: bool,
    spinner: &str,
    area: Rect,
) {
    let input_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    let title = if is_busy {
        Span::styled(
            format!(" {spinner} MauKode is thinking... (press Esc to cancel) "),
            Style::default().fg(COLOR_AMBER),
        )
    } else {
        Span::styled(" Prompt ", theme::dim_style())
    };
    let divider = Paragraph::new(Line::from(vec![
        Span::styled("─", theme::border_style()),
        title,
        Span::styled(
            "─".repeat(area.width.saturating_sub(40) as usize),
            theme::border_style(),
        ),
    ]));
    f.render_widget(divider, input_layout[0]);

    let (rows, cursor_row, cursor_column) = wrapped_input(input, cursor_pos, area.width);
    let visible_rows = input_layout[1].height as usize;
    let first_row = cursor_row.saturating_sub(visible_rows.saturating_sub(1));
    let prompt_lines: Vec<Line> = rows
        .iter()
        .skip(first_row)
        .take(visible_rows)
        .enumerate()
        .map(|(index, row)| {
            let symbol = if index + first_row == 0 {
                if is_busy {
                    "● "
                } else {
                    "❯ "
                }
            } else {
                "  "
            };
            Line::from(vec![
                Span::styled(symbol, theme::brand_style()),
                Span::styled(row.as_str(), theme::text_style()),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(prompt_lines), input_layout[1]);
    if !is_busy && input_layout[1].height > 0 {
        f.set_cursor_position((
            input_layout[1].x + cursor_column,
            input_layout[1].y + (cursor_row - first_row) as u16,
        ));
    }

    // Row 2: Contextual hotkey bar
    let hints = if is_busy {
        vec![
            Span::styled("Esc ", Style::default().fg(COLOR_AMBER)),
            Span::styled("Cancel  ", theme::dim_style()),
            Span::styled("^C ", Style::default().fg(COLOR_RED)),
            Span::styled("Quit", theme::dim_style()),
        ]
    } else {
        vec![
            Span::styled("Tab ", theme::brand_style()),
            Span::styled("Autocomplete  ", theme::dim_style()),
            Span::styled("/model ", Style::default().fg(COLOR_TOOL)),
            Span::styled("Model  ", theme::dim_style()),
            Span::styled("/provider ", Style::default().fg(COLOR_CYAN)),
            Span::styled("Provider  ", theme::dim_style()),
            Span::styled("/sessions ", Style::default().fg(COLOR_BLUE)),
            Span::styled("History  ", theme::dim_style()),
            Span::styled("/diff ", Style::default().fg(COLOR_CYAN)),
            Span::styled("Diff  ", theme::dim_style()),
            Span::styled("/undo ", Style::default().fg(COLOR_AMBER)),
            Span::styled("Undo  ", theme::dim_style()),
            Span::styled("/map ", Style::default().fg(COLOR_MINT)),
            Span::styled("Map  ", theme::dim_style()),
            Span::styled("/init ", Style::default().fg(COLOR_CYAN)),
            Span::styled("Init  ", theme::dim_style()),
            Span::styled("/mcp ", Style::default().fg(COLOR_TOOL)),
            Span::styled("MCP  ", theme::dim_style()),
            Span::styled("/compact ", Style::default().fg(COLOR_AMBER)),
            Span::styled("Compact  ", theme::dim_style()),
            Span::styled("/help ", theme::muted_style()),
            Span::styled("Commands  ", theme::dim_style()),
            Span::styled("^C ", Style::default().fg(COLOR_RED)),
            Span::styled("Quit", theme::dim_style()),
        ]
    };

    let hint_bar = Paragraph::new(Line::from(hints)).alignment(Alignment::Left);
    f.render_widget(hint_bar, input_layout[2]);
}

// -------------------------------------------------------------
// Modals: Floating Telescope Style
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

    let title = format!(
        " Select Model ─ Provider: '{}' ",
        app.config.active_provider
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_MINT))
        .title(Span::styled(title, theme::brand_style()));

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Wrapped filter search box
            Constraint::Min(4),    // Model list
            Constraint::Length(1), // Bottom hints
        ])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    // Filter box
    let filter_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border_style())
        .title(Span::styled(" Search / Filter: ", theme::muted_style()));
    let filter_line = Line::from(vec![
        Span::styled("> ", theme::brand_style()),
        Span::styled(filter, theme::text_style()),
    ]);
    let filter_width = inner[0].width.saturating_sub(2).max(1);
    let filter_rows = Paragraph::new(filter_line.clone())
        .wrap(Wrap { trim: false })
        .line_count(filter_width);
    let filter_scroll = filter_rows
        .saturating_sub(inner[0].height.saturating_sub(2) as usize)
        .min(u16::MAX as usize) as u16;
    let filter_p = Paragraph::new(filter_line)
        .block(filter_block)
        .wrap(Wrap { trim: false })
        .scroll((filter_scroll, 0));
    f.render_widget(filter_p, inner[0]);

    if is_loading {
        let loading_p = Paragraph::new(format!(
            "\n  {} Fetching models from provider API...",
            app.spinner()
        ))
        .style(theme::thinking_style())
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

            let (prefix, style) = if is_selected {
                ("❯ ", theme::selected_style())
            } else {
                ("  ", theme::text_style())
            };

            let active_badge = if is_active {
                Span::styled(" [ACTIVE]", theme::success_style())
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(*m, style),
                active_badge,
            ]))
        })
        .collect();

    let mut state = ListState::default().with_selected(if filtered_models.is_empty() {
        None
    } else {
        Some(selected_idx.min(filtered_models.len().saturating_sub(1)))
    });

    let list = List::new(items).block(Block::default().borders(Borders::NONE));

    f.render_stateful_widget(list, inner[1], &mut state);

    if filtered_models.len() > inner[1].height as usize {
        let mut scrollbar_state = ScrollbarState::new(filtered_models.len()).position(selected_idx);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme::dim_style());
        f.render_stateful_widget(scrollbar, inner[1], &mut scrollbar_state);
    }

    // Bottom modal hint
    let count_text = format!("{}/{} models", filtered_models.len(), models.len());
    let modal_footer = Line::from(vec![
        Span::styled(count_text, theme::dim_style()),
        Span::styled(" │ ", theme::border_style()),
        Span::styled("↑↓ ", theme::brand_style()),
        Span::styled("navigate  ", theme::dim_style()),
        Span::styled("Enter ", theme::brand_style()),
        Span::styled("select  ", theme::dim_style()),
        Span::styled("Esc ", Style::default().fg(COLOR_RED)),
        Span::styled("cancel", theme::dim_style()),
    ]);
    f.render_widget(
        Paragraph::new(modal_footer).alignment(Alignment::Center),
        inner[2],
    );
}

fn draw_provider_modal(f: &mut Frame, app: &App, selected_idx: usize) {
    let area = centered_rect(65, 60, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_TOOL))
        .title(Span::styled(" Select AI Provider ", theme::tool_style()));

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    let items: Vec<ListItem> = app
        .config
        .providers
        .iter()
        .enumerate()
        .map(|(idx, p)| {
            let is_active = p.id == app.config.active_provider;
            let is_selected = idx == selected_idx;

            let (prefix, style) = if is_selected {
                ("❯ ", theme::selected_style())
            } else {
                ("  ", theme::text_style())
            };

            let active_badge = if is_active {
                Span::styled(" [ACTIVE]", theme::success_style())
            } else {
                Span::raw("")
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&p.name, style),
                Span::styled(format!(" ({})", p.base_url), theme::dim_style()),
                active_badge,
            ]))
        })
        .collect();

    let mut state = ListState::default().with_selected(if app.config.providers.is_empty() {
        None
    } else {
        Some(selected_idx.min(app.config.providers.len().saturating_sub(1)))
    });

    let list = List::new(items).block(Block::default().borders(Borders::NONE));
    f.render_stateful_widget(list, inner[0], &mut state);

    if app.config.providers.len() > inner[0].height as usize {
        let mut scrollbar_state =
            ScrollbarState::new(app.config.providers.len()).position(selected_idx);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme::dim_style());
        f.render_stateful_widget(scrollbar, inner[0], &mut scrollbar_state);
    }

    let modal_footer = Line::from(vec![
        Span::styled("a ", theme::brand_style()),
        Span::styled("add new  ", theme::dim_style()),
        Span::styled("↑↓ ", theme::brand_style()),
        Span::styled("navigate  ", theme::dim_style()),
        Span::styled("Enter ", theme::brand_style()),
        Span::styled("select  ", theme::dim_style()),
        Span::styled("Esc ", Style::default().fg(COLOR_RED)),
        Span::styled("cancel", theme::dim_style()),
    ]);
    f.render_widget(
        Paragraph::new(modal_footer).alignment(Alignment::Center),
        inner[1],
    );
}

fn draw_sessions_modal(
    f: &mut Frame,
    sessions: &[maukode_core::SessionSummary],
    selected_idx: usize,
) {
    let area = centered_rect(70, 70, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_BLUE))
        .title(Span::styled(
            " Resume Session History ",
            Style::default().fg(COLOR_BLUE).add_modifier(Modifier::BOLD),
        ));

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(4), Constraint::Length(1)])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    let items: Vec<ListItem> = sessions
        .iter()
        .enumerate()
        .map(|(idx, s)| {
            let is_selected = idx == selected_idx;
            let (prefix, style) = if is_selected {
                ("❯ ", theme::selected_style())
            } else {
                ("  ", theme::text_style())
            };

            let time_str = s.updated_at.format("%m-%d %H:%M").to_string();

            let dir_label = if s.directory.is_empty() {
                String::new()
            } else {
                let p = std::path::Path::new(&s.directory);
                let folder = p
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&s.directory);
                format!(" 📁 {folder}")
            };

            ListItem::new(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&s.title, style),
                Span::styled(dir_label, Style::default().fg(COLOR_CYAN)),
                Span::styled(format!(" [{}]", s.model), theme::dim_style()),
                Span::styled(
                    format!(" ({time_str}, {} msgs)", s.message_count),
                    theme::dim_style(),
                ),
            ]))
        })
        .collect();

    let mut state = ListState::default().with_selected(if sessions.is_empty() {
        None
    } else {
        Some(selected_idx.min(sessions.len().saturating_sub(1)))
    });

    let list = List::new(items).block(Block::default().borders(Borders::NONE));
    f.render_stateful_widget(list, inner[0], &mut state);

    if sessions.len() > inner[0].height as usize {
        let mut scrollbar_state = ScrollbarState::new(sessions.len()).position(selected_idx);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme::dim_style());
        f.render_stateful_widget(scrollbar, inner[0], &mut scrollbar_state);
    }

    let modal_footer = Line::from(vec![
        Span::styled(format!("{} sessions", sessions.len()), theme::dim_style()),
        Span::styled(" │ ", theme::border_style()),
        Span::styled("↑↓ ", theme::brand_style()),
        Span::styled("navigate  ", theme::dim_style()),
        Span::styled("Enter ", theme::brand_style()),
        Span::styled("resume  ", theme::dim_style()),
        Span::styled("Esc ", Style::default().fg(COLOR_RED)),
        Span::styled("cancel", theme::dim_style()),
    ]);
    f.render_widget(
        Paragraph::new(modal_footer).alignment(Alignment::Center),
        inner[1],
    );
}

fn draw_add_provider_modal(
    f: &mut Frame,
    name: &str,
    base_url: &str,
    api_key: &str,
    default_model: &str,
    active_field: usize,
) {
    let area = centered_rect(75, 85, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_CYAN))
        .title(Span::styled(
            " Add Custom AI Provider ",
            Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
        ));

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // Name
            Constraint::Length(4), // Base URL
            Constraint::Length(4), // API Key
            Constraint::Length(4), // Default Model
            Constraint::Length(1), // Hint
        ])
        .margin(1)
        .split(area);

    f.render_widget(block, area);

    let fields = [
        ("Provider Name (e.g. DeepSeek, Local Ollama, vLLM):", name),
        (
            "Base URL (e.g. http://localhost:11434 or https://api.deepseek.com):",
            base_url,
        ),
        ("API Key (optional for local models):", api_key),
        (
            "Default Model (e.g. deepseek-chat, qwen2.5-coder):",
            default_model,
        ),
    ];

    for (idx, (label, val)) in fields.iter().enumerate() {
        let is_active = idx == active_field;
        let border_color = if is_active { COLOR_MINT } else { COLOR_BORDER };

        let field_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(
                *label,
                if is_active {
                    theme::brand_style()
                } else {
                    theme::dim_style()
                },
            ));

        let field_width = inner[idx].width.saturating_sub(2).max(1);
        let field_rows = Paragraph::new(*val)
            .wrap(Wrap { trim: false })
            .line_count(field_width);
        let field_scroll = field_rows
            .saturating_sub(inner[idx].height.saturating_sub(2) as usize)
            .min(u16::MAX as usize) as u16;
        let p = Paragraph::new(*val)
            .block(field_block)
            .wrap(Wrap { trim: false })
            .scroll((field_scroll, 0));
        f.render_widget(p, inner[idx]);
    }

    let hint = Paragraph::new(Line::from(vec![
        Span::styled("Tab ", theme::brand_style()),
        Span::styled("next  ", theme::dim_style()),
        Span::styled("Enter ", theme::brand_style()),
        Span::styled("save & activate  ", theme::dim_style()),
        Span::styled("Esc ", Style::default().fg(COLOR_RED)),
        Span::styled("cancel", theme::dim_style()),
    ]))
    .alignment(Alignment::Center);

    f.render_widget(hint, inner[4]);
}

fn draw_help_modal(f: &mut Frame) {
    let area = centered_rect(65, 55, f.area());
    f.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_MINT))
        .title(Span::styled(
            " MauKode Quick Reference ",
            theme::brand_style(),
        ));

    let help_text = vec![
        Line::from(vec![
            Span::styled(
                "/model          ",
                Style::default().fg(COLOR_TOOL).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Search and switch active LLM model", theme::text_style()),
        ]),
        Line::from(vec![
            Span::styled(
                "/provider       ",
                Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled("Switch or register an AI provider", theme::text_style()),
        ]),
        Line::from(vec![
            Span::styled(
                "/sessions       ",
                Style::default().fg(COLOR_BLUE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Browse and resume saved sessions from SQLite",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "/permission    ",
                Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Set ask/review/strict/trust tool execution mode",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "/diff           ",
                Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Inspect git shadow diff of session modifications",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "/undo [N]       ",
                Style::default()
                    .fg(COLOR_AMBER)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Roll back modified/added files by N turns (default: 1)",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "/map            ",
                Style::default().fg(COLOR_MINT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Inspect tree-sitter AST symbol skeleton of codebase",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "/init [--force] ",
                Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Scan project and generate standard AGENTS.md conventions",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "/mcp            ",
                Style::default().fg(COLOR_TOOL).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Inspect active MCP servers and registered dynamic tools",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "/compact [focus]",
                Style::default()
                    .fg(COLOR_AMBER)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Summarize older conversation turns & reclaim context tokens",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("/new            ", theme::brand_style()),
            Span::styled(
                "Start a fresh session in the current directory",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("/cd <path>      ", Style::default().fg(COLOR_CYAN)),
            Span::styled("Switch working directory on the fly", theme::text_style()),
        ]),
        Line::from(vec![
            Span::styled("/clear          ", theme::dim_style()),
            Span::styled(
                "Clear the current conversation viewport",
                theme::text_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled("/exit, /quit    ", Style::default().fg(COLOR_RED)),
            Span::styled("Exit MauKode", theme::text_style()),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("Navigation: ", theme::brand_style()),
            Span::styled(
                "PageUp / PageDn / Wheel to scroll history. Press End to resume live stream.",
                theme::muted_style(),
            ),
        ]),
    ];

    let p = Paragraph::new(help_text)
        .block(block)
        .alignment(Alignment::Left);
    f.render_widget(p, area);
}

fn draw_private_result_modal(f: &mut Frame, tool_name: &str, content: &str, scroll: usize) {
    let area = centered_rect(80, 70, f.area());
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::Yellow))
        .title(Span::styled(
            format!(" Private result: {tool_name} (hidden from AI) "),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(area);
    let paragraph = Paragraph::new(content.to_string())
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll.min(u16::MAX as usize) as u16, 0));
    f.render_widget(paragraph, area);
    if inner.height > 0 {
        f.render_widget(
            Paragraph::new("Press Esc or Enter to close; the AI continues without this result.")
                .style(theme::dim_style()),
            Rect {
                x: inner.x,
                y: inner.bottom().saturating_sub(1),
                width: inner.width,
                height: 1,
            },
        );
    }
}

fn draw_audit_modal(f: &mut Frame, content: &str, scroll: usize) {
    let area = centered_rect(80, 80, f.area());
    f.render_widget(Clear, area);
    let paragraph = Paragraph::new(content.to_string())
        .block(
            Block::default()
                .title(" Audit / provenance (Esc to close) ")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false })
        .scroll(((scroll.min(u16::MAX as usize)) as u16, 0));
    f.render_widget(paragraph, area);
}

fn draw_permission_modal(
    f: &mut Frame,
    request: &maukode_protocol::PermissionRequest,
    scroll: usize,
) -> usize {
    let screen = f.area();
    let area_width = (screen.width * 75 / 100)
        .max(64)
        .min(screen.width.saturating_sub(2));
    let cmd_text = request
        .command
        .as_deref()
        .unwrap_or(request.path.as_deref().unwrap_or(&request.description));
    let command_rows = Paragraph::new(format!("  $ {cmd_text}"))
        .wrap(Wrap { trim: false })
        .line_count(area_width.saturating_sub(4).max(1));
    let area_height = 15u16
        .saturating_add(command_rows.min(u16::MAX as usize) as u16)
        .min(screen.height.saturating_sub(2));
    let x = (screen.width.saturating_sub(area_width)) / 2;
    let y = (screen.height.saturating_sub(area_height)) / 2;
    let area = Rect::new(x, y, area_width, area_height);

    f.render_widget(Clear, area);

    let border_color = if request.risk_level == "danger" {
        COLOR_RED
    } else {
        COLOR_AMBER
    };

    let title_span = Span::styled(
        " ⚠️ Action Requires Confirmation ",
        Style::default()
            .fg(border_color)
            .add_modifier(Modifier::BOLD),
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .title(title_span);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Subtitle / intro
            Constraint::Min(3),    // Command / target box
            Constraint::Length(2), // Wrapped risk description
            Constraint::Length(0), // No spacer on narrow terminals
            Constraint::Length(4), // Action options list
        ])
        .margin(1)
        .split(block.inner(area));

    f.render_widget(block, area);

    // Intro text
    let intro = Line::from(vec![
        Span::styled("MauKode wants to execute tool ", theme::text_style()),
        Span::styled(format!("'{}'", request.tool_name), theme::tool_style()),
        Span::styled(":", theme::text_style()),
    ]);
    f.render_widget(Paragraph::new(intro), inner[0]);

    // Command / details box
    let command_view_height = inner[1].height.saturating_sub(2) as usize;
    let max_scroll = command_rows.saturating_sub(command_view_height);
    let effective_scroll = scroll.min(max_scroll);
    let cmd_para = Paragraph::new(format!("  $ {cmd_text}"))
        .scroll((effective_scroll.min(u16::MAX as usize) as u16, 0))
        .style(Style::default().fg(Color::White).bg(Color::Rgb(25, 25, 35)))
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(if max_scroll > 0 {
                    " Command ↑↓ "
                } else {
                    " Command "
                }),
        );
    f.render_widget(cmd_para, inner[1]);

    // Risk warning text
    let risk_line = Line::from(vec![
        Span::styled(
            "Risk: ",
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(&request.description, theme::text_style()),
    ]);
    f.render_widget(
        Paragraph::new(risk_line).wrap(Wrap { trim: false }),
        inner[2],
    );

    // Action options list (explicit, beautifully structured, never cut off)
    let options = vec![
        Line::from(vec![
            Span::styled(
                "  [y] ",
                Style::default()
                    .fg(COLOR_GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Yes, Allow Once        ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("• Run this command one time", theme::dim_style()),
        ]),
        Line::from(vec![
            Span::styled(
                "  [p] ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Run Privately          ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "• Show result only to you; withhold it from AI",
                theme::dim_style(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "  [a] ",
                Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "Always Allow (Session) ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("• Whitelist command for this session", theme::dim_style()),
        ]),
        Line::from(vec![
            Span::styled(
                "  [n] ",
                Style::default().fg(COLOR_RED).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "No, Reject & Abort     ",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("• Disallow execution (or press Esc)", theme::dim_style()),
        ]),
    ];
    f.render_widget(Paragraph::new(options), inner[4]);
    max_scroll
}

fn draw_diff_modal(f: &mut Frame, diff: &str, stat: &str, scroll: usize) {
    let area = centered_rect(82, 72, f.area());
    f.render_widget(Clear, area);

    let title_span = Span::styled(
        " 🔍 Session Changes Diff ",
        Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD),
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_CYAN))
        .title(title_span);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Stat summary bar
            Constraint::Length(1), // Separator
            Constraint::Min(4),    // Diff lines viewport
            Constraint::Length(1), // Footer hotkeys
        ])
        .margin(1)
        .split(block.inner(area));

    f.render_widget(block, area);

    // 1. Stat summary bar
    let stat_line = if stat.trim().is_empty() && diff.trim().is_empty() {
        Line::from(vec![Span::styled(
            "No changes detected in working directory compared to checkpoints.",
            theme::muted_style(),
        )])
    } else if !stat.trim().is_empty() {
        Line::from(vec![
            Span::styled("Summary: ", theme::brand_style()),
            Span::styled(stat.trim(), theme::text_style()),
        ])
    } else {
        Line::from(vec![Span::styled(
            "Session Checkpoint Diff",
            theme::brand_style(),
        )])
    };
    f.render_widget(Paragraph::new(stat_line), inner[0]);

    // Separator line
    let sep = Paragraph::new("─".repeat(inner[1].width as usize)).style(theme::dim_style());
    f.render_widget(sep, inner[1]);

    // 2. Diff lines viewport
    let mut diff_lines = Vec::new();
    if diff.trim().is_empty() {
        diff_lines.push(Line::from(vec![Span::styled(
            "  (Clean working tree - No modified or added files)",
            theme::dim_style(),
        )]));
    } else {
        for l in diff.lines() {
            let line_style = if l.starts_with("+++") || l.starts_with("---") {
                Style::default().fg(COLOR_FG).add_modifier(Modifier::BOLD)
            } else if l.starts_with('+') {
                theme::diff_add_style()
            } else if l.starts_with('-') {
                theme::diff_del_style()
            } else if l.starts_with("@@") {
                theme::diff_hunk_style()
            } else if l.starts_with("diff --git") {
                Style::default().fg(COLOR_MINT).add_modifier(Modifier::BOLD)
            } else {
                theme::dim_style()
            };
            diff_lines.push(Line::from(Span::styled(l, line_style)));
        }
    }

    let total_lines = diff_lines.len();
    let viewport_height = inner[2].height as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let effective_scroll = scroll.min(max_scroll);

    let p = Paragraph::new(diff_lines).scroll((effective_scroll as u16, 0));
    f.render_widget(p, inner[2]);

    if total_lines > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(effective_scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme::dim_style());
        f.render_stateful_widget(scrollbar, inner[2], &mut scrollbar_state);
    }

    // 3. Footer hotkeys
    let footer = Line::from(vec![
        Span::styled("[Esc/q] ", theme::brand_style()),
        Span::styled("Close  ", theme::dim_style()),
        Span::styled("[↑/↓ / j/k] ", theme::brand_style()),
        Span::styled("Scroll  ", theme::dim_style()),
        Span::styled("[PageUp/Dn] ", theme::brand_style()),
        Span::styled("Fast  ", theme::dim_style()),
        Span::styled(
            "[u] ",
            Style::default()
                .fg(COLOR_AMBER)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("Undo Changes (Rollback)", Style::default().fg(COLOR_AMBER)),
    ]);
    f.render_widget(
        Paragraph::new(footer).alignment(Alignment::Center),
        inner[3],
    );
}

fn draw_codebase_map_modal(f: &mut Frame, map: &str, scroll: usize) {
    let area = centered_rect(84, 76, f.area());
    f.render_widget(Clear, area);

    let title_span = Span::styled(
        " 🗺️ Codebase Map (Tree-Sitter AST Skeleton) ",
        Style::default().fg(COLOR_MINT).add_modifier(Modifier::BOLD),
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(COLOR_MINT))
        .title(title_span);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Subtitle
            Constraint::Length(1), // Separator
            Constraint::Min(4),    // Map lines viewport
            Constraint::Length(1), // Footer hotkeys
        ])
        .margin(1)
        .split(block.inner(area));

    f.render_widget(block, area);

    // 1. Subtitle
    let subtitle = Line::from(vec![
        Span::styled("Codebase Landscape: ", theme::brand_style()),
        Span::styled("Top architectural definitions, structs, classes & methods ranked by symbol centrality.", theme::muted_style()),
    ]);
    f.render_widget(Paragraph::new(subtitle), inner[0]);

    // Separator line
    let sep = Paragraph::new("─".repeat(inner[1].width as usize)).style(theme::dim_style());
    f.render_widget(sep, inner[1]);

    // 2. Map lines viewport
    let mut map_lines = Vec::new();
    if map.trim().is_empty() {
        map_lines.push(Line::from(vec![Span::styled(
            "  (No code symbols found in current workspace)",
            theme::dim_style(),
        )]));
    } else {
        for l in map.lines() {
            if l.ends_with(':') {
                // File header
                map_lines.push(Line::from(vec![
                    Span::styled("📁 ", Style::default().fg(COLOR_MINT)),
                    Span::styled(
                        l,
                        Style::default().fg(COLOR_MINT).add_modifier(Modifier::BOLD),
                    ),
                ]));
            } else if l.contains('│') {
                // Tree symbol line
                let parts: Vec<&str> = l.split('│').collect();
                let indent = parts.first().copied().unwrap_or("");
                let content = parts.get(1).copied().unwrap_or("").trim();

                let style = if content.contains("struct ")
                    || content.contains("class ")
                    || content.contains("interface ")
                    || content.contains("trait ")
                {
                    Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD)
                } else if content.contains("fn ")
                    || content.contains("def ")
                    || content.contains("func ")
                {
                    Style::default().fg(COLOR_AMBER)
                } else if content.contains("impl ") {
                    Style::default().fg(COLOR_BLUE).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(COLOR_FG)
                };

                map_lines.push(Line::from(vec![
                    Span::raw(indent),
                    Span::styled("│ ", theme::dim_style()),
                    Span::styled(content, style),
                ]));
            } else {
                map_lines.push(Line::from(Span::styled(l, theme::dim_style())));
            }
        }
    }

    let total_lines = map_lines.len();
    let viewport_height = inner[2].height as usize;
    let max_scroll = total_lines.saturating_sub(viewport_height);
    let effective_scroll = scroll.min(max_scroll);

    let p = Paragraph::new(map_lines).scroll((effective_scroll as u16, 0));
    f.render_widget(p, inner[2]);

    if total_lines > viewport_height {
        let mut scrollbar_state = ScrollbarState::new(total_lines).position(effective_scroll);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .track_symbol(Some("│"))
            .thumb_symbol("┃")
            .style(theme::dim_style());
        f.render_stateful_widget(scrollbar, inner[2], &mut scrollbar_state);
    }

    // 3. Footer hotkeys
    let footer = Line::from(vec![
        Span::styled("[Esc/q] ", theme::brand_style()),
        Span::styled("Close  ", theme::dim_style()),
        Span::styled("[↑/↓ / j/k] ", theme::brand_style()),
        Span::styled("Scroll  ", theme::dim_style()),
        Span::styled("[PageUp/Dn] ", theme::brand_style()),
        Span::styled("Fast  ", theme::dim_style()),
        Span::styled("[Home/End] ", theme::brand_style()),
        Span::styled("Jump", theme::dim_style()),
    ]);
    f.render_widget(
        Paragraph::new(footer).alignment(Alignment::Center),
        inner[3],
    );
}

#[cfg(test)]
mod chat_viewport_tests {
    use super::*;
    use crate::app::ChatItem;
    use ratatui::{backend::TestBackend, Terminal};

    fn render(
        terminal: &mut Terminal<TestBackend>,
        items: &[ChatItem],
        cache: &mut crate::render_cache::MessageRenderCache,
        scroll: &mut usize,
        max_scroll: &mut usize,
        auto_scroll: bool,
    ) -> String {
        terminal
            .draw(|frame| {
                let area = frame.area();
                draw_messages(frame, items, cache, scroll, max_scroll, auto_scroll, area);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn input_wraps_and_keeps_unicode_cursor_visible() {
        let (rows, row, column) = wrapped_input("ab界cd", 5, 6);
        assert_eq!(rows, ["ab界", "cd"]);
        assert_eq!((row, column), (1, 4));

        let mut terminal = Terminal::new(TestBackend::new(12, 5)).unwrap();
        terminal
            .draw(|frame| {
                draw_input(
                    frame,
                    "a long input that wraps",
                    23,
                    false,
                    "⠋",
                    frame.area(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let screen = (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            screen.contains("input that") && screen.contains("wraps"),
            "{screen}"
        );
    }

    #[test]
    fn provider_field_and_permission_command_show_wrapped_tail() {
        let mut terminal = Terminal::new(TestBackend::new(100, 35)).unwrap();
        terminal
            .draw(|frame| {
                draw_add_provider_modal(
                    frame,
                    "Local",
                    "https://example.test/a/very/long/provider/path/with/final-segment",
                    "",
                    "model",
                    1,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let provider_screen: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(
            provider_screen.contains("final-seg") && provider_screen.contains("ment"),
            "{provider_screen}"
        );

        let request = maukode_protocol::PermissionRequest {
            request_id: "req".into(),
            session_id: "session".into(),
            tool_name: "bash".into(),
            command: Some(format!(
                "{} final-command-tail",
                "echo long words ".repeat(12)
            )),
            path: None,
            risk_level: "warning".into(),
            description: "A command with a longer explanation that should wrap".into(),
        };
        terminal
            .draw(|frame| {
                draw_permission_modal(frame, &request, 0);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let permission_screen: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(permission_screen.contains("final-command-tail"));
        assert!(permission_screen.contains("should wrap"));
    }

    #[test]
    fn wrapped_message_can_be_scrolled_to_its_end() {
        let mut terminal = Terminal::new(TestBackend::new(18, 5)).unwrap();
        let items = vec![ChatItem {
            role: ChatRole::Assistant,
            content: "alpha beta gamma delta epsilon zeta omega".into(),
        }];
        let mut cache = crate::render_cache::MessageRenderCache::default();
        let (mut scroll, mut max_scroll) = (0, 0);
        let top = render(
            &mut terminal,
            &items,
            &mut cache,
            &mut scroll,
            &mut max_scroll,
            false,
        );
        assert!(top.contains("alpha"), "{top}");
        assert!(max_scroll > 0);

        let bottom = render(
            &mut terminal,
            &items,
            &mut cache,
            &mut scroll,
            &mut max_scroll,
            true,
        );
        assert_eq!(scroll, max_scroll);
        assert!(bottom.contains("omega"), "{bottom}");
    }

    #[test]
    fn streamed_chat_remains_renderable_under_ci_budget() {
        use std::time::{Duration, Instant};

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut items = Vec::new();
        let mut cache = crate::render_cache::MessageRenderCache::default();
        let (mut scroll, mut max_scroll) = (0, 0);
        let started = Instant::now();
        for batch in 0..40 {
            for _ in 0..100 {
                crate::app::append_stream_delta(
                    &mut items,
                    ChatRole::Assistant,
                    "streamed words ".into(),
                );
            }
            let screen = render(
                &mut terminal,
                &items,
                &mut cache,
                &mut scroll,
                &mut max_scroll,
                true,
            );
            assert!(screen.contains("streamed"), "batch {batch}");
        }
        assert_eq!(items.len(), 1);
        assert_eq!(scroll, max_scroll);
        assert!(
            started.elapsed() < Duration::from_secs(15),
            "4,000 chunks and 40 redraws took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn long_history_reaches_both_ends_without_overscrolling() {
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let items: Vec<_> = (0..2_000)
            .map(|index| ChatItem {
                role: ChatRole::Assistant,
                content: format!("message {index:04}"),
            })
            .collect();
        let mut cache = crate::render_cache::MessageRenderCache::default();
        let (mut scroll, mut max_scroll) = (0, 0);
        let bottom = render(
            &mut terminal,
            &items,
            &mut cache,
            &mut scroll,
            &mut max_scroll,
            true,
        );
        assert!(bottom.contains("message 1999"), "{bottom}");
        assert_eq!(scroll, max_scroll);
        assert!(max_scroll > 5_000);

        scroll = 0;
        let top = render(
            &mut terminal,
            &items,
            &mut cache,
            &mut scroll,
            &mut max_scroll,
            false,
        );
        assert!(top.contains("message 0000"), "{top}");
        assert_eq!(scroll, 0);

        scroll = usize::MAX;
        let clamped = render(
            &mut terminal,
            &items,
            &mut cache,
            &mut scroll,
            &mut max_scroll,
            false,
        );
        assert_eq!(scroll, max_scroll);
        assert!(clamped.contains("message 1999"), "{clamped}");
    }
}
