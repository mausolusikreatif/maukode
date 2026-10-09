use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PreviewRequest {
    pub port: Option<u16>,
    /// Serialized SandboxMode (`off`, `path_jail`, `bubblewrap`, or `auto`).
    pub sandbox_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PreviewInfo {
    pub status: String,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub ready: bool,
    pub error: Option<String>,
}
