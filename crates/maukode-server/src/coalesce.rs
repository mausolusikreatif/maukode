//! Merging of consecutive streaming deltas before they are published.
//!
//! Providers stream a few characters per event. Publishing each one costs an SSE frame per
//! subscriber and a slot in the broadcast channel and in the replay log, which a long reply would
//! otherwise overflow, so a client reconnecting mid-turn could not rebuild the message.

use maukode_protocol::LLMEvent;
use std::time::Duration;
use tokio::sync::mpsc::UnboundedReceiver;

/// Longest a delta waits for more of the same stream before it is published.
pub const DELTA_WINDOW: Duration = Duration::from_millis(50);

fn is_delta(event: &LLMEvent) -> bool {
    matches!(
        event,
        LLMEvent::TextDelta { .. }
            | LLMEvent::ReasoningDelta { .. }
            | LLMEvent::ToolCallDelta { .. }
    )
}

/// Append `next` to `pending` when both are deltas of the same stream; otherwise hand `next` back.
fn merge_delta(pending: &mut LLMEvent, next: LLMEvent) -> Option<LLMEvent> {
    match (pending, next) {
        (LLMEvent::TextDelta { text }, LLMEvent::TextDelta { text: more })
        | (LLMEvent::ReasoningDelta { text }, LLMEvent::ReasoningDelta { text: more }) => {
            text.push_str(&more);
            None
        }
        (
            LLMEvent::ToolCallDelta { id, delta },
            LLMEvent::ToolCallDelta {
                id: next_id,
                delta: more,
            },
        ) if *id == next_id => {
            delta.push_str(&more);
            None
        }
        (_, next) => Some(next),
    }
}

/// Runner events with consecutive deltas of one stream merged into a single event. A delta waits
/// at most [`DELTA_WINDOW`] for more; any other event is passed on at once.
pub struct CoalescingReceiver {
    rx: UnboundedReceiver<LLMEvent>,
    held: Option<LLMEvent>,
}

impl CoalescingReceiver {
    pub fn new(rx: UnboundedReceiver<LLMEvent>) -> Self {
        Self { rx, held: None }
    }

    pub async fn recv(&mut self) -> Option<LLMEvent> {
        let mut event = match self.held.take() {
            Some(event) => event,
            None => self.rx.recv().await?,
        };
        if !is_delta(&event) {
            return Some(event);
        }
        let deadline = tokio::time::Instant::now() + DELTA_WINDOW;
        while let Ok(Some(next)) = tokio::time::timeout_at(deadline, self.rx.recv()).await {
            if let Some(other) = merge_delta(&mut event, next) {
                self.held = Some(other);
                break;
            }
        }
        Some(event)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    fn text(text: &str) -> LLMEvent {
        LLMEvent::TextDelta { text: text.into() }
    }

    fn tool_delta(id: &str, delta: &str) -> LLMEvent {
        LLMEvent::ToolCallDelta {
            id: id.into(),
            delta: delta.into(),
        }
    }

    #[tokio::test]
    async fn consecutive_deltas_of_one_stream_are_merged() {
        let (tx, rx) = mpsc::unbounded_channel();
        for event in [
            text("Hel"),
            text("lo"),
            LLMEvent::ReasoningDelta { text: "x".into() },
            tool_delta("a", "{\"p\""),
            tool_delta("a", ":1}"),
            tool_delta("b", "{}"),
            LLMEvent::Finish {
                reason: maukode_protocol::FinishReason::ToolUse,
            },
            text("!"),
        ] {
            tx.send(event).unwrap();
        }
        drop(tx);

        let mut rx = CoalescingReceiver::new(rx);
        let mut received = Vec::new();
        while let Some(event) = rx.recv().await {
            received.push(event);
        }
        assert_eq!(
            received,
            [
                text("Hello"),
                LLMEvent::ReasoningDelta { text: "x".into() },
                tool_delta("a", "{\"p\":1}"),
                tool_delta("b", "{}"),
                LLMEvent::Finish {
                    reason: maukode_protocol::FinishReason::ToolUse,
                },
                text("!"),
            ]
        );
    }

    #[tokio::test]
    async fn a_delta_is_published_when_no_more_text_arrives() {
        let (tx, rx) = mpsc::unbounded_channel();
        tx.send(text("partial")).unwrap();
        let mut rx = CoalescingReceiver::new(rx);
        // The sender stays open: the delta must still come out after the window.
        assert_eq!(rx.recv().await, Some(text("partial")));
        drop(tx);
        assert_eq!(rx.recv().await, None);
    }
}
