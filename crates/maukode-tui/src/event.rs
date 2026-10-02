use crossterm::event::{self, Event as CrosstermEvent, KeyEvent};
use maukode_protocol::LLMEvent;
use std::time::Duration;
use tokio::sync::mpsc;

pub enum AppEvent {
    Input(KeyEvent),
    Tick,
    LLM(LLMEvent),
    TurnFinished(Result<(), String>),
    ModelsFetched {
        provider_id: String,
        models: Result<Vec<String>, String>,
    },
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
    _tx: mpsc::UnboundedSender<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> (Self, mpsc::UnboundedSender<AppEvent>) {
        let (tx, rx) = mpsc::unbounded_channel();
        let event_tx = tx.clone();

        // Terminal event listener thread
        tokio::spawn(async move {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    if let Ok(CrosstermEvent::Key(key)) = event::read() {
                        let _ = event_tx.send(AppEvent::Input(key));
                    }
                }
                let _ = event_tx.send(AppEvent::Tick);
            }
        });

        (Self { rx, _tx: tx.clone() }, tx)
    }

    pub async fn next(&mut self) -> Option<AppEvent> {
        self.rx.recv().await
    }
}
