//! Per-item render cache for the chat viewport.
//!
//! `draw_messages` used to rebuild and word-wrap the *entire* conversation on every frame
//! (see the discussion that produced `benches/render_messages.rs`): both `build_message_lines`
//! (markdown parsing) and the `Paragraph` wrap pass scale with total history size, and while
//! streaming, `Paragraph`'s scroll-skip loop re-wraps everything above the viewport too. That
//! makes every frame cost grow with the whole conversation instead of just the part that
//! changed or is visible.
//!
//! This cache keeps, per `ChatItem`, the already-styled `Line`s and their wrapped height at the
//! current width. On each frame only items whose content actually changed (in practice: the
//! message currently streaming in) are rebuilt; everything else is reused. The viewport query
//! then only concatenates and wraps the handful of items that actually overlap what's on
//! screen, so render cost tracks viewport size, not history length.

use crate::app::ChatItem;
use crate::ui::build_item_lines;
use ratatui::{
    text::Line,
    widgets::{Paragraph, Wrap},
};

struct CacheEntry {
    signature: ChatItem,
    lines: Vec<Line<'static>>,
    width: u16,
    height: usize,
}

#[derive(Default)]
pub struct MessageRenderCache {
    entries: Vec<CacheEntry>,
}

impl MessageRenderCache {
    /// Reconcile the cache against the current items at `width`, recomputing only entries
    /// whose content or role changed (or whose cached width is stale after a resize).
    pub fn sync(&mut self, items: &[ChatItem], width: u16) {
        self.entries.truncate(items.len());
        while self.entries.len() < items.len() {
            self.entries.push(CacheEntry {
                signature: ChatItem {
                    role: crate::app::ChatRole::System,
                    content: String::new(),
                },
                lines: Vec::new(),
                width: 0,
                height: 0,
            });
        }

        for (entry, item) in self.entries.iter_mut().zip(items.iter()) {
            if entry.width != width || entry.signature != *item {
                let lines = build_item_lines(item);
                let height = Paragraph::new(lines.clone())
                    .wrap(Wrap { trim: false })
                    .line_count(width.max(1));
                entry.signature = item.clone();
                entry.lines = lines;
                entry.width = width;
                entry.height = height;
            }
        }
    }

    /// Total wrapped height across all items, as of the last `sync`.
    pub fn total_height(&self) -> usize {
        self.entries.iter().map(|e| e.height).sum()
    }

    /// The lines needed to render rows `[scroll, scroll + viewport_height)` of the full
    /// wrapped conversation, plus the local scroll offset (within those lines) to hand to
    /// `Paragraph::scroll`. Only the items overlapping that row range are included, so the
    /// returned `Paragraph` only ever has to wrap a viewport's worth of content.
    pub fn visible_lines(
        &self,
        scroll: usize,
        viewport_height: usize,
    ) -> (Vec<Line<'static>>, u16) {
        let end = scroll.saturating_add(viewport_height);
        let mut lines = Vec::new();
        let mut local_scroll = 0u16;
        let mut cursor = 0usize;
        let mut started = false;

        for entry in &self.entries {
            let entry_end = cursor + entry.height;
            if entry_end > scroll && cursor < end {
                if !started {
                    local_scroll = scroll.saturating_sub(cursor).min(u16::MAX as usize) as u16;
                    started = true;
                }
                lines.extend(entry.lines.iter().cloned());
            }
            cursor = entry_end;
            if cursor >= end {
                break;
            }
        }

        (lines, local_scroll)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ChatRole;

    fn item(content: &str) -> ChatItem {
        ChatItem {
            role: ChatRole::User,
            content: content.to_string(),
        }
    }

    #[test]
    fn total_height_matches_full_rebuild() {
        let items = vec![item("hello"), item("world, this is a longer line of text")];
        let mut cache = MessageRenderCache::default();
        cache.sync(&items, 40);

        let full = crate::ui::build_message_lines(&items);
        let expected = Paragraph::new(full)
            .wrap(Wrap { trim: false })
            .line_count(40);
        assert_eq!(cache.total_height(), expected);
    }

    #[test]
    fn unchanged_items_keep_their_cached_entry() {
        let mut items = vec![item("first"), item("second")];
        let mut cache = MessageRenderCache::default();
        cache.sync(&items, 40);
        let height_before = cache.entries[0].height;
        let lines_before = cache.entries[0].lines.len();

        items[1].content.push_str(" grew");
        cache.sync(&items, 40);

        assert_eq!(cache.entries[0].height, height_before);
        assert_eq!(cache.entries[0].lines.len(), lines_before);
    }

    #[test]
    fn visible_lines_covers_requested_window() {
        let items: Vec<_> = (0..20).map(|i| item(&format!("line {i}"))).collect();
        let mut cache = MessageRenderCache::default();
        cache.sync(&items, 40);
        let total = cache.total_height();

        let (window, local_scroll) = cache.visible_lines(total - 3, 10);
        assert!(!window.is_empty());
        assert!((local_scroll as usize) < window.len());
    }
}
