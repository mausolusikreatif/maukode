//! Server-Sent Events plumbing shared by the per-session and global streams.

use crate::state::{EventChannel, SeqEvent, ServerState};
use axum::{
    extract::{Query, State},
    http::HeaderMap,
    response::sse::{Event, KeepAlive, Sse},
};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::broadcast::{error::RecvError, Receiver};

#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    /// Replay events with a sequence number greater than this (alternative to `Last-Event-ID`).
    pub since: Option<u64>,
}

/// Resume point from `?since=` or the `Last-Event-ID` header.
pub fn resume_point(query: &EventsQuery, headers: &HeaderMap) -> Option<u64> {
    query.since.or_else(|| {
        headers
            .get("last-event-id")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
    })
}

struct StreamState<T> {
    rx: Receiver<SeqEvent<T>>,
    backlog: VecDeque<SeqEvent<T>>,
    last: u64,
    channel: Arc<EventChannel<T>>,
}

fn to_sse_event<T: Serialize>(ev: &SeqEvent<T>) -> Event {
    Event::default()
        .id(ev.seq.to_string())
        .data(serde_json::to_string(&ev.event).unwrap_or_default())
}

/// Replay (if `since` is set) then follow live events. Slow consumers are caught up from the
/// replay log instead of silently losing events.
pub fn sse_response<T>(
    channel: Arc<EventChannel<T>>,
    since: Option<u64>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>>
where
    T: Clone + Serialize + Send + 'static,
{
    let (backlog, rx, last) = channel.subscribe(since);
    let init = StreamState {
        rx,
        backlog: backlog.into(),
        last,
        channel,
    };
    let stream = futures::stream::unfold(init, |mut st| async move {
        loop {
            if let Some(ev) = st.backlog.pop_front() {
                st.last = ev.seq;
                return Some((Ok(to_sse_event(&ev)), st));
            }
            match st.rx.recv().await {
                Ok(ev) if ev.seq <= st.last => continue,
                Ok(ev) => {
                    st.last = ev.seq;
                    return Some((Ok(to_sse_event(&ev)), st));
                }
                Err(RecvError::Lagged(_)) => st.backlog = st.channel.replay_after(st.last).into(),
                Err(RecvError::Closed) => return None,
            }
        }
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// `GET /api/v1/events` — activity across all sessions (turns, permission prompts, lifecycle).
pub async fn global_events(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let since = resume_point(&query, &headers);
    sse_response(Arc::clone(&state.global_events), since)
}
