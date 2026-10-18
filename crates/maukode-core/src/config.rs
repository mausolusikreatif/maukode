use crate::permission::PermissionMode;
use anyhow::Result;
use maukode_llm::{AnthropicClient, LLMClient, OpenAIClient};
use maukode_tools::{LawangSettings, SandboxMode};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

/// Agent loop limits applied to every turn started through the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct AgentConfig {
    #[serde(default = "default_max_steps")]
    pub max_steps: usize,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Post-turn reflection: the agent evaluates its performance and extracts lessons.
    /// See [`crate::reflection::ReflectionConfig`].
    #[serde(default)]
    pub reflection: crate::reflection::ReflectionConfig,
    /// Post-turn memory consolidation: detect and reconcile near-duplicate memories,
    /// and synthesise higher-level abstractions.
    /// See [`crate::consolidation::ConsolidationConfig`].
    #[serde(default)]
    pub consolidation: crate::consolidation::ConsolidationConfig,
}

fn default_max_steps() -> usize {
    2000
}

fn default_max_tokens() -> u32 {
    8192
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_steps: default_max_steps(),
            max_tokens: default_max_tokens(),
            reflection: crate::reflection::ReflectionConfig::default(),
            consolidation: crate::consolidation::ConsolidationConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenAiCompatible,
    ChatGpt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub permission_mode: PermissionMode,
    #[serde(default = "default_true")]
    pub jail_path_traversal: bool,
    #[serde(default)]
    pub allowed_external_paths: Vec<PathBuf>,
    #[serde(default)]
    pub sandbox_mode: SandboxMode,
    #[serde(default = "default_whitelist")]
    pub bash_whitelist: Vec<String>,
}

fn default_true() -> bool {
    true
}

fn default_whitelist() -> Vec<String> {
    vec![
        "cargo *".to_string(),
        "git status".to_string(),
        "git diff".to_string(),
        "npm test".to_string(),
    ]
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            permission_mode: PermissionMode::Ask,
            jail_path_traversal: true,
            allowed_external_paths: Vec::new(),
            sandbox_mode: SandboxMode::default(),
            bash_whitelist: default_whitelist(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebSearchConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_search_provider")]
    pub provider: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default = "default_max_search_results")]
    pub max_results: usize,
}

fn default_search_provider() -> String {
    "duckduckgo".to_string()
}

fn default_max_search_results() -> usize {
    5
}

impl Default for WebSearchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: "duckduckgo".to_string(),
            api_key: None,
            endpoint: None,
            max_results: 5,
        }
    }
}

/// Where the bash tool's commands run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionBackendKind {
    /// As local processes (with the sandbox mode under `security`).
    #[default]
    Host,
    /// In a cell of a lawang daemon: bubblewrap, gVisor or a Firecracker VM.
    Lawang,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default)]
    pub backend: ExecutionBackendKind,
    #[serde(default)]
    pub lawang: LawangSettings,
}

impl ExecutionConfig {
    /// What the UI labels a session: `host`, or the isolation level of the lawang cell.
    pub fn label(&self) -> &'static str {
        match self.backend {
            ExecutionBackendKind::Host => "host",
            ExecutionBackendKind::Lawang => self.lawang.isolation.as_str(),
        }
    }

    /// Reasons these settings cannot work, found without contacting the daemon.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.backend != ExecutionBackendKind::Lawang {
            return problems;
        }
        let url = self.lawang.url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            problems.push("the lawang URL must start with http:// or https://".to_string());
        }
        let has_image = self
            .lawang
            .image
            .as_deref()
            .is_some_and(|i| !i.trim().is_empty());
        if self.lawang.isolation == maukode_tools::CellIsolation::Vm && !has_image {
            problems.push(
                "a VM has its own kernel and no host libraries, so it needs an image (for example alpine:3.20)"
                    .to_string(),
            );
        }
        if self.lawang.memory_mb == Some(0) {
            problems.push("the memory limit must be at least 1 MiB".to_string());
        }
        problems
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub active_provider: String,
    pub active_model: String,
    pub providers: Vec<ProviderConfig>,
    #[serde(default)]
    pub security: SecurityConfig,
    #[serde(default, rename = "mcpServers", alias = "mcp_servers")]
    pub mcp_servers: std::collections::HashMap<String, McpServerConfig>,
    #[serde(default)]
    pub web_search: WebSearchConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub execution: ExecutionConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let openai_key = std::env::var("OPENAI_API_KEY").ok();
        let llmkita_key = std::env::var("LLMKITA_API_KEY").ok();

        Self {
            active_provider: "llmkita".to_string(),
            active_model: "auto".to_string(),
            mcp_servers: std::collections::HashMap::new(),
            web_search: WebSearchConfig::default(),
            execution: ExecutionConfig::default(),
            providers: vec![
                ProviderConfig {
                    id: "llmkita".to_string(),
                    name: "llm-kita".to_string(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: "https://api.llm-kita.com/api/v1".to_string(),
                    api_key: llmkita_key,
                    default_model: Some("auto".to_string()),
                },
                ProviderConfig {
                    id: "anthropic".to_string(),
                    name: "Anthropic".to_string(),
                    kind: ProviderKind::Anthropic,
                    base_url: "https://api.anthropic.com".to_string(),
                    api_key: anthropic_key,
                    default_model: Some("claude-3-7-sonnet-20250219".to_string()),
                },
                ProviderConfig {
                    id: "openai".to_string(),
                    name: "OpenAI".to_string(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: "https://api.openai.com".to_string(),
                    api_key: openai_key,
                    default_model: Some("gpt-4o".to_string()),
                },
                ProviderConfig {
                    id: "chatgpt".to_string(),
                    name: "ChatGPT plan".to_string(),
                    kind: ProviderKind::ChatGpt,
                    base_url: "https://api.openai.com/v1".to_string(),
                    api_key: None,
                    default_model: None,
                },
                ProviderConfig {
                    id: "ollama".to_string(),
                    name: "Ollama (Local)".to_string(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: "http://localhost:11434".to_string(),
                    api_key: None,
                    default_model: Some("qwen2.5-coder:latest".to_string()),
                },
                ProviderConfig {
                    id: "openrouter".to_string(),
                    name: "OpenRouter".to_string(),
                    kind: ProviderKind::OpenAiCompatible,
                    base_url: "https://openrouter.ai/api".to_string(),
                    api_key: std::env::var("OPENROUTER_API_KEY").ok(),
                    default_model: Some("anthropic/claude-3.7-sonnet".to_string()),
                },
            ],
            security: SecurityConfig::default(),
            agent: AgentConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        if let Some(mut dir) = crate::paths::config_dir() {
            dir.push("maukode");
            dir.push("config.json");
            dir
        } else if let Some(mut dir) = dirs::home_dir() {
            dir.push(".maukode");
            dir.push("config.json");
            dir
        } else {
            PathBuf::from(".maukode.config.json")
        }
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut cfg) = serde_json::from_str::<AppConfig>(&content) {
                    if cfg.get_provider("chatgpt").is_none() {
                        cfg.providers.push(ProviderConfig {
                            id: "chatgpt".into(),
                            name: "ChatGPT plan".into(),
                            kind: ProviderKind::ChatGpt,
                            base_url: "https://api.openai.com/v1".into(),
                            api_key: None,
                            default_model: None,
                        });
                    }
                    // Update keys from env if missing
                    for p in &mut cfg.providers {
                        if p.api_key.is_none() {
                            let variable = match p.id.as_str() {
                                "anthropic" => Some("ANTHROPIC_API_KEY"),
                                "openai" => Some("OPENAI_API_KEY"),
                                "openrouter" => Some("OPENROUTER_API_KEY"),
                                "llmkita" => Some("LLMKITA_API_KEY"),
                                _ => None,
                            };
                            if let Some(variable) = variable {
                                p.api_key = std::env::var(variable).ok();
                            }
                        }
                    }
                    let _ = cfg.save();
                    return cfg;
                }
            }
        }
        let default = Self::default();
        let _ = default.save();
        default
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    pub fn get_provider(&self, id: &str) -> Option<&ProviderConfig> {
        self.providers
            .iter()
            .find(|p| p.id.eq_ignore_ascii_case(id))
    }

    pub fn get_active_provider(&self) -> Option<&ProviderConfig> {
        self.get_provider(&self.active_provider)
    }

    pub fn add_or_update_provider(&mut self, provider: ProviderConfig) {
        if let Some(idx) = self.providers.iter().position(|p| p.id == provider.id) {
            self.providers[idx] = provider;
        } else {
            self.providers.push(provider);
        }
        let _ = self.save();
    }

    /// Remove a provider. The active provider cannot be removed; returns whether it existed.
    pub fn remove_provider(&mut self, id: &str) -> Result<bool> {
        if self.active_provider == id {
            anyhow::bail!("Cannot remove the active provider; switch to another one first");
        }
        let before = self.providers.len();
        self.providers.retain(|p| p.id != id);
        let removed = self.providers.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn set_active_provider(&mut self, id: &str) -> Result<()> {
        let provider = self
            .get_provider(id)
            .ok_or_else(|| anyhow::anyhow!("Provider '{id}' not found"))?
            .clone();

        if provider.kind == ProviderKind::ChatGpt
            && crate::chatgpt_auth::ChatGptAuth::new()?.status()?.is_none()
        {
            anyhow::bail!(
                "Sign in with ChatGPT first (Settings → ChatGPT, or `maukode auth login`)"
            );
        }

        self.active_provider = provider.id;
        if let Some(model) = provider.default_model {
            self.active_model = model;
        }
        let _ = self.save();
        Ok(())
    }

    pub fn set_active_model(&mut self, model: &str) {
        self.active_model = model.to_string();
        if let Some(p) = self
            .providers
            .iter_mut()
            .find(|p| p.id == self.active_provider)
        {
            p.default_model = Some(model.to_string());
        }
        let _ = self.save();
    }

    pub fn create_active_llm_client(&self) -> Result<Arc<dyn LLMClient>> {
        self.create_llm_client_for(&self.active_provider)
    }

    /// Build a client for a specific provider (used for per-session provider/model pinning).
    pub fn create_llm_client_for(&self, provider_id: &str) -> Result<Arc<dyn LLMClient>> {
        let provider = self
            .get_provider(provider_id)
            .ok_or_else(|| anyhow::anyhow!("Provider '{provider_id}' not found"))?;

        match provider.kind {
            ProviderKind::ChatGpt => {
                let auth = crate::chatgpt_auth::ChatGptAuth::new()?;
                Ok(Arc::new(crate::chatgpt_client::ChatGptClient::new(auth)))
            }
            ProviderKind::Anthropic => {
                let key = provider
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                    .unwrap_or_default();
                let client = AnthropicClient::new(key).with_base_url(&provider.base_url);
                Ok(Arc::new(client))
            }
            ProviderKind::OpenAiCompatible => {
                let key = provider
                    .api_key
                    .clone()
                    .or_else(|| std::env::var("OPENAI_API_KEY").ok())
                    .unwrap_or_default();
                let client = OpenAIClient::new(key).with_base_url(&provider.base_url);
                Ok(Arc::new(client))
            }
        }
    }

    /// Dynamically fetch available models from the provider API
    pub async fn fetch_models_for_provider(&self, provider_id: &str) -> Result<Vec<String>> {
        let provider = self
            .get_provider(provider_id)
            .ok_or_else(|| anyhow::anyhow!("Provider '{provider_id}' not found"))?;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()?;

        match provider.kind {
            ProviderKind::ChatGpt => {
                let auth = crate::chatgpt_auth::ChatGptAuth::new()?;
                let token = auth.access_token().await?;
                let response = client
                    .get("https://api.openai.com/v1/models")
                    .bearer_auth(token)
                    .send()
                    .await?
                    .error_for_status()?;
                let data: serde_json::Value = response.json().await?;
                let models = data["models"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("ChatGPT model catalog missing"))?
                    .iter()
                    .filter(|model| model["visibility"] == "list")
                    .filter_map(|model| model["slug"].as_str().map(str::to_owned))
                    .collect();
                Ok(models)
            }
            ProviderKind::Anthropic => {
                let url = format!("{}/v1/models", provider.base_url.trim_end_matches('/'));
                let key = provider.api_key.as_deref().unwrap_or_default();

                let resp = client
                    .get(&url)
                    .header("x-api-key", key)
                    .header("anthropic-version", "2023-06-01")
                    .send()
                    .await;

                if let Ok(res) = resp {
                    if res.status().is_success() {
                        if let Ok(val) = res.json::<serde_json::Value>().await {
                            if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
                                let mut models: Vec<String> = arr
                                    .iter()
                                    .filter_map(|m| {
                                        m.get("id")
                                            .and_then(|id| id.as_str())
                                            .map(|s| s.to_string())
                                    })
                                    .collect();
                                models.sort();
                                if !models.is_empty() {
                                    return Ok(models);
                                }
                            }
                        }
                    }
                }

                // Fallback known models for Anthropic
                Ok(vec![
                    "claude-3-7-sonnet-20250219".to_string(),
                    "claude-3-5-sonnet-20241022".to_string(),
                    "claude-3-5-haiku-20241022".to_string(),
                    "claude-3-opus-20240229".to_string(),
                ])
            }
            ProviderKind::OpenAiCompatible => {
                let base = provider.base_url.trim_end_matches('/');
                let url = if base.ends_with("/models") {
                    base.to_string()
                } else if base.ends_with("/v1") {
                    format!("{base}/models")
                } else {
                    format!("{base}/v1/models")
                };

                let mut req = client.get(&url);
                if let Some(ref key) = provider.api_key {
                    if !key.is_empty() {
                        req = req.header("authorization", format!("Bearer {key}"));
                    }
                }

                let resp = req.send().await;
                if let Ok(res) = resp {
                    if res.status().is_success() {
                        if let Ok(val) = res.json::<serde_json::Value>().await {
                            // Standard OpenAI {"data": [{"id": "..."}]}
                            if let Some(arr) = val.get("data").and_then(|d| d.as_array()) {
                                let mut models: Vec<String> = arr
                                    .iter()
                                    .filter_map(|m| {
                                        m.get("id")
                                            .and_then(|id| id.as_str())
                                            .map(|s| s.to_string())
                                    })
                                    .collect();
                                models.sort();
                                if !models.is_empty() {
                                    return Ok(models);
                                }
                            }
                            // Ollama tags {"models": [{"name": "..."}]}
                            if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
                                let mut models: Vec<String> = arr
                                    .iter()
                                    .filter_map(|m| {
                                        m.get("name")
                                            .and_then(|n| n.as_str())
                                            .map(|s| s.to_string())
                                    })
                                    .collect();
                                models.sort();
                                if !models.is_empty() {
                                    return Ok(models);
                                }
                            }
                        }
                    }
                }

                // If Ollama, try /api/tags
                if provider.id == "ollama" {
                    let ollama_url = format!("{base}/api/tags");
                    if let Ok(res) = client.get(&ollama_url).send().await {
                        if let Ok(val) = res.json::<serde_json::Value>().await {
                            if let Some(arr) = val.get("models").and_then(|m| m.as_array()) {
                                let mut models: Vec<String> = arr
                                    .iter()
                                    .filter_map(|m| {
                                        m.get("name")
                                            .and_then(|n| n.as_str())
                                            .map(|s| s.to_string())
                                    })
                                    .collect();
                                models.sort();
                                if !models.is_empty() {
                                    return Ok(models);
                                }
                            }
                        }
                    }
                }

                // Fallback default list
                if provider.id == "openai" {
                    Ok(vec![
                        "gpt-4o".to_string(),
                        "gpt-4o-mini".to_string(),
                        "o1".to_string(),
                        "o3-mini".to_string(),
                    ])
                } else if provider.id == "ollama" {
                    Ok(vec![
                        "qwen2.5-coder:latest".to_string(),
                        "llama3.3:latest".to_string(),
                        "deepseek-r1:latest".to_string(),
                    ])
                } else {
                    Ok(vec!["default".to_string()])
                }
            }
        }
    }

    /// Configure web search and documentation fetching tools in a ToolRegistry based on AppConfig
    pub fn configure_web_search_in_registry(&self, registry: &mut maukode_tools::ToolRegistry) {
        if !self.web_search.enabled {
            registry.unregister("web_search");
            registry.unregister("search_web");
            registry.unregister("fetch_web_content");
            registry.unregister("fetch_url");
            registry.unregister("read_url");
        } else {
            let mut search_tool =
                maukode_tools::WebSearchTool::new().with_provider(&self.web_search.provider);

            if let Some(ref key) = self.web_search.api_key {
                search_tool = search_tool.with_api_key(Some(key.clone()));
            }
            if let Some(ref ep) = self.web_search.endpoint {
                search_tool = search_tool.with_endpoint(Some(ep.clone()));
            }

            registry.register(Arc::new(search_tool));
            registry.register_alias("search_web", "web_search");
            registry.register(Arc::new(maukode_tools::FetchWebContentTool::new()));
            registry.register_alias("fetch_url", "fetch_web_content");
            registry.register_alias("read_url", "fetch_web_content");
        }
    }
}

#[cfg(test)]
mod execution_tests {
    use super::*;
    use maukode_tools::CellIsolation;

    fn lawang(isolation: CellIsolation, image: Option<&str>) -> ExecutionConfig {
        ExecutionConfig {
            backend: ExecutionBackendKind::Lawang,
            lawang: LawangSettings {
                isolation,
                image: image.map(String::from),
                ..LawangSettings::default()
            },
        }
    }

    #[test]
    fn a_config_file_from_before_this_feature_still_runs_commands_on_the_host() {
        let old = r#"{"active_provider":"x","active_model":"y","providers":[]}"#;
        let config: AppConfig = serde_json::from_str(old).unwrap();
        assert_eq!(config.execution.backend, ExecutionBackendKind::Host);
        assert_eq!(config.execution.label(), "host");
        assert!(config.execution.problems().is_empty());
    }

    #[test]
    fn the_label_names_the_isolation_level_of_a_lawang_cell() {
        assert_eq!(ExecutionConfig::default().label(), "host");
        assert_eq!(lawang(CellIsolation::Process, None).label(), "process");
        assert_eq!(lawang(CellIsolation::Sandbox, None).label(), "sandbox");
        assert_eq!(lawang(CellIsolation::Vm, Some("alpine:3.20")).label(), "vm");
    }

    #[test]
    fn a_vm_without_an_image_is_a_problem_but_the_other_levels_are_not() {
        assert!(lawang(CellIsolation::Process, None).problems().is_empty());
        assert!(lawang(CellIsolation::Sandbox, None).problems().is_empty());
        let vm = lawang(CellIsolation::Vm, None).problems();
        assert_eq!(vm.len(), 1);
        assert!(vm[0].contains("needs an image"), "{vm:?}");
        assert!(
            lawang(CellIsolation::Vm, Some("   ")).problems().len() == 1,
            "blank is no image"
        );
        assert!(lawang(CellIsolation::Vm, Some("alpine:3.20"))
            .problems()
            .is_empty());
    }

    #[test]
    fn a_bad_url_or_memory_limit_is_reported_and_the_host_is_never_a_problem() {
        let mut c = lawang(CellIsolation::Process, None);
        c.lawang.url = "unix:/run/lawangd.sock".into();
        c.lawang.memory_mb = Some(0);
        let problems = c.problems();
        assert_eq!(problems.len(), 2, "{problems:?}");
        // Settings that would not work do not matter while commands run on the host.
        c.backend = ExecutionBackendKind::Host;
        assert!(c.problems().is_empty());
    }

    #[test]
    fn the_execution_section_round_trips_and_the_token_is_not_printed() {
        let mut c = lawang(CellIsolation::Sandbox, Some("alpine:3.20"));
        c.lawang.token = Some("s3cret-token".into());
        let json = serde_json::to_string(&c).unwrap();
        let back: ExecutionConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        assert!(
            json.contains(r#""backend":"lawang""#) && json.contains(r#""isolation":"sandbox""#),
            "{json}"
        );
        assert!(
            !format!("{c:?}").contains("s3cret-token"),
            "Debug output must redact the token"
        );
    }
}
