//! HTTP client settings shared by the provider adapters.

use std::time::Duration;

/// Time allowed to open a connection to the provider.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Longest silence allowed between two reads of a response. Every streamed chunk resets it, so
/// long generations are fine, but a half-open connection fails the attempt (and the runner
/// retries it) instead of hanging the turn forever.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// A client with connect and read timeouts. No total timeout: a streamed reply may legitimately
/// take many minutes.
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .unwrap_or_else(|e| {
            tracing::warn!("Could not build the HTTP client with timeouts ({e}); using defaults");
            reqwest::Client::new()
        })
}
