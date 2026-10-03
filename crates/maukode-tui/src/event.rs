use crossterm::event::{self, Event as CrosstermEvent, KeyEvent, MouseEvent, MouseEventKind};
use maukode_protocol::LLMEvent;
use std::time::Duration;
use tokio::sync::mpsc;

pub enum AppEvent {
    Input(KeyEvent),
    Mouse(MouseEvent),
    Tick,
    LLM(LLMEvent),
    TurnFinished(Result<(), String>),
    ModelsFetched {
        provider_id: String,
        models: Result<Vec<String>, String>,
    },
    DiffFetched(maukode_protocol::DiffResponse),
    UndoCompleted(Result<String, String>),
    CodebaseMapFetched(Result<String, String>),
    InitProjectCompleted(Result<maukode_protocol::InitProjectResponse, String>),
    McpStatusFetched(Result<maukode_protocol::McpListResponse, String>),
    SessionCompacted(Result<maukode_protocol::CompactResponse, String>),
    AuditLoaded(Result<String, String>),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    _tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> (Self, mpsc::UnboundedSender<AppEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();

        // 1. Dedicated OS thread for blocking crossterm input polling.
        // This prevents blocking Tokio worker threads and prevents 100% CPU lockups.
        let input_tx = tx.clone();
        std::thread::spawn(move || {
            loop {
                // Poll with a short timeout so the thread can exit when the app exits
                if event::poll(Duration::from_millis(100)).unwrap_or(false) {
                    if let Ok(crossterm_event) = event::read() {
                        let to_send = match crossterm_event {
                            CrosstermEvent::Key(key) => Some(AppEvent::Input(key)),
                            CrosstermEvent::Mouse(mouse) => {
                                // Filter out raw mouse movements (Moved, Drag, Down, Up).
                                // Only forward ScrollUp and ScrollDown to prevent flooding hundreds of events/sec!
                                match mouse.kind {
                                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                                        Some(AppEvent::Mouse(mouse))
                                    }
                                    _ => None,
                                }
                            }
                            _ => None,
                        };

                        if let Some(app_event) = to_send {
                            if input_tx.send(app_event).is_err() {
                                break;
                            }
                        }
                    }
                }

                if input_tx.is_closed() {
                    break;
                }
            }
        });

        // 2. Steady tick generator running on a fixed timer interval,
        // independent of mouse/keyboard input bursts.
        let tick_tx = tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(tick_rate);
            loop {
                interval.tick().await;
                if tick_tx.send(AppEvent::Tick).is_err() {
                    break;
                }
            }
        });

        (
            Self {
                rx,
                _tx: tx.clone(),
            },
            tx,
        )
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }

    pub fn try_next(&mut self) -> Option<AppEvent> {
        self.rx.try_recv().ok()
    }
}
