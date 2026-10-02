use ratatui::style::{Color, Modifier, Style};

pub const COLOR_MINT: Color = Color::Rgb(0, 245, 160);
pub const COLOR_TEAL_DIM: Color = Color::Rgb(0, 180, 130);
pub const COLOR_FG: Color = Color::Rgb(226, 232, 240);
pub const COLOR_MUTED: Color = Color::Rgb(100, 116, 139);
pub const COLOR_DIM: Color = Color::Rgb(71, 85, 105);
pub const COLOR_BORDER: Color = Color::Rgb(51, 65, 85);
pub const COLOR_SURFACE: Color = Color::Rgb(15, 23, 42);
pub const COLOR_SURFACE_HOVER: Color = Color::Rgb(30, 41, 59);
pub const COLOR_TOOL: Color = Color::Rgb(167, 139, 250);
pub const COLOR_AMBER: Color = Color::Rgb(245, 158, 11);
pub const COLOR_GREEN: Color = Color::Rgb(16, 185, 129);
pub const COLOR_RED: Color = Color::Rgb(239, 68, 68);
pub const COLOR_BLUE: Color = Color::Rgb(96, 165, 250);
pub const COLOR_CYAN: Color = Color::Rgb(56, 189, 248);

pub fn brand_style() -> Style {
    Style::default().fg(COLOR_MINT).add_modifier(Modifier::BOLD)
}

pub fn text_style() -> Style {
    Style::default().fg(COLOR_FG)
}

pub fn muted_style() -> Style {
    Style::default().fg(COLOR_MUTED)
}

pub fn dim_style() -> Style {
    Style::default().fg(COLOR_DIM)
}

pub fn border_style() -> Style {
    Style::default().fg(COLOR_BORDER)
}

pub fn tool_style() -> Style {
    Style::default().fg(COLOR_TOOL).add_modifier(Modifier::BOLD)
}

pub fn thinking_style() -> Style {
    Style::default()
        .fg(COLOR_AMBER)
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
    Style::default().fg(COLOR_GREEN)
}

pub fn diff_del_style() -> Style {
    Style::default().fg(COLOR_RED)
}

pub fn diff_hunk_style() -> Style {
    Style::default().fg(COLOR_CYAN).add_modifier(Modifier::BOLD)
}

pub fn selected_style() -> Style {
    Style::default()
        .fg(COLOR_MINT)
        .bg(COLOR_SURFACE_HOVER)
        .add_modifier(Modifier::BOLD)
}
