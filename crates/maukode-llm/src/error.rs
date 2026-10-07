use thiserror::Error;

#[derive(Debug, Error)]
pub enum LLMError {
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("API error (status {status}): {body}")]
    Api { status: u16, body: String },

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Streaming error: {0}")]
    Stream(String),
}

impl LLMError {
    /// Whether the failure looks transient (network blip, gateway EOF/reset,
    /// rate limit, overloaded backend) and is worth retrying. Used by the
    /// OpenAI-compatible client and the agent runner to smooth over flaky
    /// OpenAI-compatible gateways instead of failing the whole turn.
    pub fn is_transient(&self) -> bool {
        match self {
            LLMError::Network(_) => true,
            LLMError::Api { status, .. } => *status == 429 || (500..600).contains(status),
            LLMError::Stream(msg) => {
                let m = msg.to_lowercase();
                [
                    "eof",
                    "connection",
                    "reset",
                    "broken pipe",
                    "incomplete",
                    "unexpected eof",
                    "timeout",
                    "timed out",
                    "temporarily",
                    "overloaded",
                    "rate limit",
                    "rate_limit",
                    "try again",
                    "service unavailable",
                    "bad gateway",
                    "gateway timeout",
                    "internal error",
                    "server error",
                    "429",
                    "500",
                    "502",
                    "503",
                    "529",
                ]
                .iter()
                .any(|s| m.contains(*s))
            }
            LLMError::Parse(_) | LLMError::Config(_) => false,
        }
    }
}
