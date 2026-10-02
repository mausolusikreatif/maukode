use std::path::PathBuf;
use std::sync::Arc;
use anyhow::Result;
use maukode_llm::{AnthropicClient, LLMClient, OpenAIClient};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Anthropic,
    OpenAiCompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub api_key: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub active_provider: String,
    pub active_model: String,
    pub providers: Vec<ProviderConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let anthropic_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let openai_key = std::env::var("OPENAI_API_KEY").ok();

        Self {
            active_provider: "anthropic".to_string(),
            active_model: "claude-3-7-sonnet-20250219".to_string(),
            providers: vec![
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
        }
    }
}

impl AppConfig {
    pub fn config_path() -> PathBuf {
        if let Some(mut dir) = dirs::config_dir() {
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
                    // Update keys from env if missing
                    for p in &mut cfg.providers {
                        if p.api_key.is_none() {
                            if p.id == "anthropic" {
                                p.api_key = std::env::var("ANTHROPIC_API_KEY").ok();
                            } else if p.id == "openai" {
                                p.api_key = std::env::var("OPENAI_API_KEY").ok();
                            }
                        }
                    }
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
        self.providers.iter().find(|p| p.id.eq_ignore_ascii_case(id))
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

    pub fn set_active_provider(&mut self, id: &str) -> Result<()> {
        let provider = self
            .get_provider(id)
            .ok_or_else(|| anyhow::anyhow!("Provider '{id}' not found"))?
            .clone();

        self.active_provider = provider.id;
        if let Some(model) = provider.default_model {
            self.active_model = model;
        }
        let _ = self.save();
        Ok(())
    }

    pub fn set_active_model(&mut self, model: &str) {
        self.active_model = model.to_string();
        if let Some(p) = self.providers.iter_mut().find(|p| p.id == self.active_provider) {
            p.default_model = Some(model.to_string());
        }
        let _ = self.save();
    }

    pub fn create_active_llm_client(&self) -> Result<Arc<dyn LLMClient>> {
        let provider = self
            .get_active_provider()
            .ok_or_else(|| anyhow::anyhow!("No active provider configured"))?;

        match provider.kind {
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
                                    .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
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
                let url = format!("{base}/v1/models");

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
                                    .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(|s| s.to_string()))
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
                                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
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
                                    .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
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
}
