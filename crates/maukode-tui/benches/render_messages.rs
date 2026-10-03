//! Baseline/target benchmarks for the chat render hot path (`ui::build_message_lines` plus
//! the `Paragraph` wrap pass used to compute scroll bounds). See AGENTS.md discussion on TUI
//! CPU usage: these two steps currently redo work over the *entire* conversation on every
//! frame, so cost should (before the fix) scale linearly with history size, and streaming-turn
//! cost should be roughly quadratic in the number of deltas appended during one turn.
//!
//! Run: `cargo bench -p maukode-tui -- --save-baseline before-cache`
//! Compare after a fix: `cargo bench -p maukode-tui -- --baseline before-cache`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use maukode_tui::app::{ChatItem, ChatRole};
use maukode_tui::render_cache::MessageRenderCache;
use maukode_tui::ui::build_message_lines;
use ratatui::widgets::{Paragraph, Wrap};

const TERMINAL_WIDTH: u16 = 100;
const VIEWPORT_HEIGHT: usize = 24;

fn assistant_markdown(seed: usize) -> String {
    format!(
        "## Step {seed}\n\n\
        Here is a short explanation paragraph that mimics a typical streamed assistant reply, \
        long enough to need wrapping across a normal terminal width.\n\n\
        - first point about change {seed}\n\
        - second point with `inline code`\n\
        - third point referencing a file at src/lib.rs:{seed}\n\n\
        ```rust\n\
        fn example_{seed}() -> usize {{\n\
        +    {seed}\n\
        -    0\n\
        }}\n\
        ```\n"
    )
}

fn build_history(num_turns: usize) -> Vec<ChatItem> {
    let mut items = Vec::with_capacity(num_turns * 2);
    for i in 0..num_turns {
        items.push(ChatItem {
            role: ChatRole::User,
            content: format!("Please apply change number {i} and explain the reasoning."),
        });
        items.push(ChatItem {
            role: ChatRole::Assistant,
            content: assistant_markdown(i),
        });
    }
    items
}

/// Cost of one `terminal.draw()`-equivalent frame *without* the render cache: rebuild all lines
/// (markdown parse included) then run the same wrap pass `draw_messages` used to use for scroll
/// math. This is the pre-fix code path, kept as the "before" reference.
fn uncached_frame_cost(items: &[ChatItem]) {
    let lines = build_message_lines(items);
    let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
    let _ = criterion::black_box(paragraph.line_count(TERMINAL_WIDTH));
}

/// Cost of one frame through `MessageRenderCache`, exactly as `draw_messages` drives it: sync
/// the cache, total the wrapped height, then pull out just the visible window (here: the
/// bottom of the conversation, mirroring `auto_scroll = true`).
fn cached_frame_cost(cache: &mut MessageRenderCache, items: &[ChatItem]) {
    cache.sync(items, TERMINAL_WIDTH);
    let total = cache.total_height();
    let scroll = total.saturating_sub(VIEWPORT_HEIGHT);
    let _ = criterion::black_box(cache.visible_lines(scroll, VIEWPORT_HEIGHT));
}

/// Isolates the two sub-costs of `one_frame_cost` so we know which part a fix must target.
fn bench_frame_cost_breakdown(c: &mut Criterion) {
    let items = build_history(250);

    let mut group = c.benchmark_group("frame_cost_breakdown_at_250_turns");
    group.bench_function("build_lines_only", |b| {
        b.iter(|| criterion::black_box(build_message_lines(&items)));
    });
    group.bench_function("wrap_line_count_only", |b| {
        let lines = build_message_lines(&items);
        b.iter(|| {
            let paragraph = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
            criterion::black_box(paragraph.line_count(TERMINAL_WIDTH))
        });
    });
    group.finish();
}

fn bench_frame_cost_by_history_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_cost_by_history_size");
    for &turns in &[5usize, 25, 50, 150, 250] {
        let items = build_history(turns);
        group.bench_with_input(BenchmarkId::new("uncached", turns), &items, |b, items| {
            b.iter(|| uncached_frame_cost(items));
        });
    }
    group.finish();
}

/// Same scenarios as `bench_frame_cost_by_history_size`, but through a cache that was already
/// warmed once (steady state: redrawing a frame where nothing changed, e.g. a tick with no new
/// tokens, or scrolling). This is the number the fix is supposed to flatten.
fn bench_cached_frame_cost_by_history_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("frame_cost_by_history_size");
    for &turns in &[5usize, 25, 50, 150, 250] {
        let items = build_history(turns);
        let mut cache = MessageRenderCache::default();
        cached_frame_cost(&mut cache, &items); // warm up, not timed
        group.bench_with_input(
            BenchmarkId::new("cached_steady_state", turns),
            &items,
            |b, items| {
                b.iter(|| cached_frame_cost(&mut cache, items));
            },
        );
    }
    group.finish();
}

/// Simulates one full streaming turn: a history of `base_turns` already-finished messages, plus
/// one assistant message that grows by `num_deltas` chunks. Without the cache, every delta
/// triggers a full `draw()`-equivalent frame over the *whole* history, so this models the real
/// per-turn CPU cost users would feel while a response streams in.
fn uncached_streaming_turn_cost(base_turns: usize, num_deltas: usize) {
    let mut items = build_history(base_turns);
    items.push(ChatItem {
        role: ChatRole::Assistant,
        content: String::new(),
    });
    for i in 0..num_deltas {
        items.last_mut().unwrap().content.push_str(&format!(
            "token-{i} chunk of streamed text that keeps the message growing realistically. "
        ));
        uncached_frame_cost(&items);
    }
}

/// Same scenario, but with one `MessageRenderCache` kept alive across the whole turn (as the
/// real `App` does): the `base_turns` history is only ever cached, not rebuilt, so the cost
/// should track `num_deltas` (the size of the one growing message) rather than `base_turns`.
fn cached_streaming_turn_cost(base_turns: usize, num_deltas: usize) {
    let mut items = build_history(base_turns);
    items.push(ChatItem {
        role: ChatRole::Assistant,
        content: String::new(),
    });
    let mut cache = MessageRenderCache::default();
    for i in 0..num_deltas {
        items.last_mut().unwrap().content.push_str(&format!(
            "token-{i} chunk of streamed text that keeps the message growing realistically. "
        ));
        cached_frame_cost(&mut cache, &items);
    }
}

fn bench_streaming_turn_by_history_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("streaming_turn_cost_by_history_size");
    group.sample_size(20);
    for &base_turns in &[5usize, 25, 50, 150, 250] {
        group.bench_with_input(
            BenchmarkId::new("uncached", base_turns),
            &base_turns,
            |b, &base_turns| {
                b.iter(|| uncached_streaming_turn_cost(base_turns, 50));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("cached", base_turns),
            &base_turns,
            |b, &base_turns| {
                b.iter(|| cached_streaming_turn_cost(base_turns, 50));
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_frame_cost_breakdown,
    bench_frame_cost_by_history_size,
    bench_cached_frame_cost_by_history_size,
    bench_streaming_turn_by_history_size
);
criterion_main!(benches);
