use crate::theme::{
    self, COLOR_AMBER, COLOR_BLUE, COLOR_CYAN, COLOR_MINT, COLOR_MUTED, COLOR_SURFACE_HOVER,
};
use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

/// Render GitHub-flavoured Markdown into terminal-native Ratatui lines.
///
/// The parser deliberately accepts incomplete input: assistant responses are rendered while they
/// are still streaming, so an unclosed emphasis marker or code fence must remain harmless.
pub fn render(markdown: &str) -> Vec<Line<'static>> {
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES;
    let parser = Parser::new_ext(markdown, options);
    let mut renderer = Renderer::default();

    for event in parser {
        renderer.event(event);
    }
    renderer.finish()
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    styles: Vec<Style>,
    lists: Vec<ListState>,
    quote_depth: usize,
    in_code_block: bool,
    code_language: Option<String>,
    in_table: bool,
    table_cell: usize,
}

struct ListState {
    next: Option<u64>,
}

impl Renderer {
    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(text.as_ref()),
            Event::Code(code) => self.push(
                code.into_string(),
                Style::default().fg(COLOR_AMBER).bg(COLOR_SURFACE_HOVER),
            ),
            Event::SoftBreak => self.line_break(),
            Event::HardBreak => self.line_break(),
            Event::Rule => {
                self.flush();
                self.lines.push(Line::from(Span::styled(
                    "  ────────────────────────────────",
                    Style::default().fg(COLOR_MUTED),
                )));
            }
            Event::TaskListMarker(checked) => {
                self.push(if checked { "[x] " } else { "[ ] " }, theme::text_style());
            }
            Event::FootnoteReference(reference) => {
                self.push(format!("[{}]", reference), Style::default().fg(COLOR_BLUE))
            }
            Event::Html(html) | Event::InlineHtml(html) => self.text(html.as_ref()),
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                self.push(math.into_string(), Style::default().fg(COLOR_AMBER))
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.ensure_prefix(),
            Tag::Heading { level, .. } => {
                self.flush();
                self.ensure_prefix();
                let color = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => COLOR_MINT,
                    _ => COLOR_CYAN,
                };
                self.styles
                    .push(Style::default().fg(color).add_modifier(Modifier::BOLD));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.in_code_block = true;
                self.code_language = match kind {
                    CodeBlockKind::Fenced(language) if !language.is_empty() => {
                        Some(language.into_string())
                    }
                    _ => None,
                };
                if let Some(language) = self.code_language.clone() {
                    self.lines.push(Line::from(vec![
                        Span::raw("  "),
                        Span::styled(language, Style::default().fg(COLOR_MUTED)),
                    ]));
                }
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(ListState { next: start });
            }
            Tag::Item => {
                self.flush();
                let depth = self.lists.len();
                let marker = self.lists.last_mut().map_or_else(
                    || "• ".to_string(),
                    |list| match list.next.as_mut() {
                        Some(next) => {
                            let marker = format!("{next}. ");
                            *next += 1;
                            marker
                        }
                        None => "• ".to_string(),
                    },
                );
                self.spans.push(Span::raw("  ".repeat(depth)));
                self.spans
                    .push(Span::styled(marker, Style::default().fg(COLOR_CYAN)));
            }
            Tag::Emphasis => self
                .styles
                .push(Style::default().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self
                .styles
                .push(Style::default().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self
                .styles
                .push(Style::default().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { .. } => self.styles.push(
                Style::default()
                    .fg(COLOR_BLUE)
                    .add_modifier(Modifier::UNDERLINED),
            ),
            Tag::Image { .. } => self.push("[image: ", Style::default().fg(COLOR_BLUE)),
            Tag::Table(_) => {
                self.flush();
                self.in_table = true;
            }
            Tag::TableHead | Tag::TableRow => {
                self.flush();
                self.table_cell = 0;
                self.spans.push(Span::raw("  │ "));
            }
            Tag::TableCell => {
                if self.table_cell > 0 {
                    self.spans.push(Span::raw(" │ "));
                }
                self.table_cell += 1;
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.styles.pop();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quote_depth = self.quote_depth.saturating_sub(1);
            }
            TagEnd::CodeBlock => {
                self.flush();
                self.in_code_block = false;
                self.code_language = None;
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link => {
                self.styles.pop();
            }
            TagEnd::Image => self.push("]", Style::default().fg(COLOR_BLUE)),
            TagEnd::Table => {
                self.flush();
                self.in_table = false;
            }
            TagEnd::TableHead => {
                self.spans.push(Span::raw(" │"));
                self.flush();
            }
            TagEnd::TableRow => {
                self.spans.push(Span::raw(" │"));
                self.flush();
            }
            _ => {}
        }
    }

    fn text(&mut self, text: &str) {
        if self.in_code_block {
            for (index, line) in text.split('\n').enumerate() {
                if index > 0 {
                    self.flush();
                }
                if !line.is_empty() {
                    self.ensure_code_prefix();
                    let style = if line.starts_with('+') {
                        theme::diff_add_style()
                    } else if line.starts_with('-') {
                        theme::diff_del_style()
                    } else if line.starts_with("@@") {
                        theme::diff_hunk_style()
                    } else {
                        Style::default().fg(COLOR_MINT)
                    };
                    self.push(line.to_string(), style);
                }
            }
        } else {
            self.ensure_prefix();
            self.push(text.to_string(), self.current_style());
        }
    }

    fn current_style(&self) -> Style {
        self.styles
            .iter()
            .copied()
            .fold(theme::text_style(), |style, next| style.patch(next))
    }

    fn ensure_prefix(&mut self) {
        if self.spans.is_empty() {
            self.spans.push(Span::raw("  "));
            for _ in 0..self.quote_depth {
                self.spans
                    .push(Span::styled("│ ", Style::default().fg(COLOR_MUTED)));
            }
        }
    }

    fn ensure_code_prefix(&mut self) {
        if self.spans.is_empty() {
            self.spans.push(Span::styled(
                "  │ ",
                Style::default().fg(COLOR_MUTED).bg(COLOR_SURFACE_HOVER),
            ));
        }
    }

    fn push(&mut self, content: impl Into<String>, style: Style) {
        self.spans.push(Span::styled(content.into(), style));
    }

    fn line_break(&mut self) {
        self.flush();
        if !self.in_code_block {
            self.ensure_prefix();
        }
    }

    fn flush(&mut self) {
        if !self.spans.is_empty() {
            self.lines.push(Line::from(std::mem::take(&mut self.spans)));
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush();
        self.lines
    }
}

#[cfg(test)]
mod tests {
    use super::render;

    fn text(lines: &[ratatui::text::Line<'_>]) -> String {
        lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn strips_markers_and_renders_gfm_constructs() {
        let output = render(
            "## Status\n\n- [x] **Done**\n- ~~old~~ `new`\n\n| A | B |\n|---|---|\n| 1 | 2 |",
        );
        let rendered = text(&output);
        assert!(rendered.contains("Status"));
        assert!(!rendered.contains("##"));
        assert!(rendered.contains("[x] Done"));
        assert!(rendered.contains("old new"));
        assert!(rendered.contains("│ A │ B │"));
    }

    #[test]
    fn heading_style_does_not_leak_into_subsequent_text() {
        let output = render("# Heading\n\nNormal text after heading.");
        let normal_line = output
            .iter()
            .find(|line| {
                line.spans
                    .iter()
                    .any(|span| span.content.contains("Normal text"))
            })
            .expect("should find the normal text line");

        let normal_span = normal_line
            .spans
            .iter()
            .find(|span| span.content.contains("Normal text"))
            .expect("should find the normal text span");

        // The heading pushed BOLD + fg(MINT) onto the style stack.
        // If it leaks, the normal text will also be BOLD + MINT.
        assert!(
            !normal_span
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD),
            "normal text after heading should not be bold (heading style leaked)"
        );
        assert_eq!(
            normal_span.style.fg,
            Some(ratatui::style::Color::Rgb(226, 232, 240)),
            "normal text after heading should have default fg, not heading color"
        );
    }

    #[test]
    fn tolerates_an_unclosed_streaming_code_fence() {
        let output = render("text\n\n```rust\nfn main() {");
        let rendered = text(&output);
        assert!(rendered.contains("rust"));
        assert!(rendered.contains("fn main() {"));
    }
}
