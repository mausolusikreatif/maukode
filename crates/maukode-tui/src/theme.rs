//! TUI colors, taken from the web UI's dark tokens (`apps/web/src/index.css`, `--paper`,
//! `--surface`, `--ink`, ...). A terminal cannot read `prefers-color-scheme`, so the TUI always
//! uses the dark set and leaves the terminal background alone.
//!
//! Each accent has one meaning, the same as in the web UI:
//! - `COLOR_PRIMARY` (cobalt): the focused or active item and the action that sends work.
//! - `COLOR_HOLD` (amber): the agent is waiting for you (approvals, queued prompts, warnings).
//! - `COLOR_RED`: refuse, fail, or a deleted line.
//! - `COLOR_GREEN`: success, or an added line.
//!
//! Everything else is `COLOR_FG` or `COLOR_MUTED`, and hierarchy comes from weight.

use ratatui::style::{Color, Modifier, Style};

/// `--ink`
pub const COLOR_FG: Color = Color::Rgb(0xe6, 0xea, 0xed);
/// `--ink-2`
pub const COLOR_MUTED: Color = Color::Rgb(0x93, 0xa0, 0xab);
/// `--line`
pub const COLOR_BORDER: Color = Color::Rgb(0x2b, 0x34, 0x3c);
/// `--surface`
pub const COLOR_SURFACE: Color = Color::Rgb(0x17, 0x1c, 0x21);
/// Background of the selected row; the web UI marks selection with ink, so this is `--line`.
pub const COLOR_SELECTED: Color = Color::Rgb(0x2b, 0x34, 0x3c);
/// `--cobalt`
pub const COLOR_PRIMARY: Color = Color::Rgb(0x6c, 0x86, 0xff);
/// `--hold`
pub const COLOR_HOLD: Color = Color::Rgb(0xf0, 0xb4, 0x29);
/// `--green`
pub const COLOR_GREEN: Color = Color::Rgb(0x4c, 0xc3, 0x8a);
/// `--red`
pub const COLOR_RED: Color = Color::Rgb(0xff, 0x6b, 0x5e);
/// `--add-bg`
pub const COLOR_ADD_BG: Color = Color::Rgb(0x12, 0x30, 0x1f);
/// `--del-bg`
pub const COLOR_DEL_BG: Color = Color::Rgb(0x3a, 0x1a, 0x17);

pub fn brand_style() -> Style {
    Style::default().fg(COLOR_FG).add_modifier(Modifier::BOLD)
}

pub fn text_style() -> Style {
    Style::default().fg(COLOR_FG)
}

pub fn muted_style() -> Style {
    Style::default().fg(COLOR_MUTED)
}

pub fn dim_style() -> Style {
    Style::default().fg(COLOR_MUTED)
}

pub fn border_style() -> Style {
    Style::default().fg(COLOR_BORDER)
}

pub fn tool_style() -> Style {
    Style::default()
        .fg(COLOR_MUTED)
        .add_modifier(Modifier::BOLD)
}

pub fn thinking_style() -> Style {
    Style::default()
        .fg(COLOR_MUTED)
        .add_modifier(Modifier::ITALIC)
}

pub fn success_style() -> Style {
    Style::default()
        .fg(COLOR_GREEN)
        .add_modifier(Modifier::BOLD)
}

pub fn error_style() -> Style {
    Style::default().fg(COLOR_RED).add_modifier(Modifier::BOLD)
}

pub fn diff_add_style() -> Style {
    Style::default().fg(COLOR_GREEN).bg(COLOR_ADD_BG)
}

pub fn diff_del_style() -> Style {
    Style::default().fg(COLOR_RED).bg(COLOR_DEL_BG)
}

pub fn diff_hunk_style() -> Style {
    Style::default()
        .fg(COLOR_MUTED)
        .add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .fg(COLOR_FG)
        .bg(COLOR_SELECTED)
        .add_modifier(Modifier::BOLD)
}
