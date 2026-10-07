use crate::skill::SkillDefinition;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PluginMcpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: Option<String>,
    #[serde(default)]
    pub is_enabled: bool,
    #[serde(default)]
    pub skills: Vec<SkillDefinition>,
    #[serde(default)]
    pub mcp_servers: HashMap<String, PluginMcpServerConfig>,
    #[serde(default)]
    pub rules: Vec<String>,
    pub path: Option<String>,
}
