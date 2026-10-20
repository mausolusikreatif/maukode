use anyhow::{anyhow, Result};
use eventsource_stream::Eventsource;
use futures::{Stream, StreamExt};
use maukode_core::SessionSummary;
use maukode_protocol::{
    LLMEvent, Message, PluginManifest, SessionId, SessionMetadata, SessionState, SkillDefinition,
};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone)]
pub struct ServerClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub id: SessionId,
    pub directory: String,
    pub state: SessionState,
    pub messages: Vec<Message>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub usage: maukode_protocol::SessionUsage,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProvidersResponse {
    pub active_provider: String,
    pub active_model: String,
    pub providers: Vec<maukode_core::ProviderConfig>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ModelsResponse {
    pub provider: String,
    pub models: Vec<String>,
}

impl ServerClient {
    pub fn new(base_url: impl Into<String>, auth_token: Option<String>) -> Result<Self> {
        let mut base = base_url.into();
        if base.ends_with('/') {
            base.pop();
        }

        let mut headers = HeaderMap::new();
        if let Some(token) = auth_token {
            let auth_val = format!("Bearer {token}");
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&auth_val)?);
        }

        let http = reqwest::Client::builder()
            .default_headers(headers)
            .build()?;

        Ok(Self {
            base_url: base,
            http,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn health(&self) -> Result<bool> {
        let resp = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    pub async fn list_providers(&self) -> Result<ProvidersResponse> {
        let resp = self
            .http
            .get(format!("{}/api/v1/providers", self.base_url))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list providers: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn set_active_provider(&self, provider_id: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/providers/active", self.base_url))
            .json(&serde_json::json!({ "provider_id": provider_id }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to set active provider: {err}"));
        }
        Ok(())
    }

    pub async fn add_or_update_provider(
        &self,
        provider: &maukode_core::ProviderConfig,
    ) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/providers", self.base_url))
            .json(provider)
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to update provider: {err}"));
        }
        Ok(())
    }

    pub async fn list_models(&self, provider: Option<&str>) -> Result<Vec<String>> {
        let mut req = self.http.get(format!("{}/api/v1/models", self.base_url));
        if let Some(p) = provider {
            req = req.query(&[("provider", p)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list models: {err}"));
        }
        let res: ModelsResponse = resp.json().await?;
        Ok(res.models)
    }

    pub async fn set_active_model(&self, model: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/models/active", self.base_url))
            .json(&serde_json::json!({ "model": model }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to set active model: {err}"));
        }
        Ok(())
    }

    pub async fn set_session_model(
        &self,
        id: SessionId,
        model: &str,
        provider_id: Option<&str>,
    ) -> Result<()> {
        let body = serde_json::json!({
            "model": model,
            "provider_id": provider_id,
        });
        let resp = self
            .http
            .put(format!("{}/api/v1/sessions/{id}/model", self.base_url))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to set session model: {err}"));
        }
        Ok(())
    }

    pub async fn set_agent_config(
        &self,
        max_steps: Option<usize>,
        max_tokens: Option<u32>,
        reflection: Option<maukode_core::ReflectionConfig>,
    ) -> Result<()> {
        let body = serde_json::json!({
            "max_steps": max_steps,
            "max_tokens": max_tokens,
            "reflection": reflection,
        });
        let resp = self
            .http
            .post(format!("{}/api/v1/config/agent", self.base_url))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to set agent config: {err}"));
        }
        Ok(())
    }

    pub async fn list_sessions(
        &self,
        directory: Option<&Path>,
        all: bool,
    ) -> Result<Vec<SessionSummary>> {
        let mut req = self.http.get(format!("{}/api/v1/sessions", self.base_url));
        if all {
            req = req.query(&[("all", "true")]);
        } else if let Some(dir) = directory {
            let dir_str = dir.to_string_lossy().to_string();
            req = req.query(&[("directory", dir_str)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list sessions: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn create_session(
        &self,
        directory: Option<&Path>,
        title: Option<&str>,
    ) -> Result<SessionMetadata> {
        self.create_session_with_options(directory, title, None, None)
            .await
    }

    pub async fn create_session_with_options(
        &self,
        directory: Option<&Path>,
        title: Option<&str>,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> Result<SessionMetadata> {
        let body = serde_json::json!({
            "directory": directory.map(|d| d.to_string_lossy().to_string()),
            "title": title,
            "provider": provider,
            "model": model,
        });
        let resp = self
            .http
            .post(format!("{}/api/v1/sessions", self.base_url))
            .json(&body)
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to create session: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_session(&self, id: SessionId) -> Result<SessionDetail> {
        let resp = self
            .http
            .get(format!("{}/api/v1/sessions/{id}", self.base_url))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to get session {id}: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn submit_prompt(&self, id: SessionId, prompt: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/sessions/{id}/prompt", self.base_url))
            .json(&serde_json::json!({ "prompt": prompt }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to submit prompt: {err}"));
        }
        Ok(())
    }

    /// Add guidance to the running turn (`POST /sessions/{id}/steer`).
    pub async fn steer_prompt(&self, id: SessionId, prompt: &str) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/sessions/{id}/steer", self.base_url))
            .json(&serde_json::json!({ "prompt": prompt }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to steer: {err}"));
        }
        Ok(())
    }

    pub async fn cancel_prompt(&self, id: SessionId) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/sessions/{id}/cancel", self.base_url))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to cancel prompt: {err}"));
        }
        Ok(())
    }

    pub async fn subscribe_events(
        &self,
        id: SessionId,
    ) -> Result<std::pin::Pin<Box<dyn Stream<Item = Result<LLMEvent>> + Send>>> {
        let resp = self
            .http
            .get(format!("{}/api/v1/sessions/{id}/events", self.base_url))
            .header("Accept", "text/event-stream")
            .send()
            .await?;

        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to connect to SSE stream: {err}"));
        }

        let event_source = resp.bytes_stream().eventsource();
        let stream = event_source.filter_map(|item| async move {
            match item {
                Ok(ev) => {
                    if ev.data.trim().is_empty() {
                        None
                    } else {
                        match serde_json::from_str::<LLMEvent>(&ev.data) {
                            Ok(llm_event) => Some(Ok(llm_event)),
                            Err(e) => Some(Err(anyhow!("Failed to parse SSE event: {e}"))),
                        }
                    }
                }
                Err(e) => Some(Err(anyhow!("SSE stream error: {e}"))),
            }
        });

        Ok(Box::pin(stream))
    }

    pub async fn set_permission_mode(&self, mode: maukode_core::PermissionMode) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/permissions/mode", self.base_url))
            .json(&serde_json::json!({ "mode": mode }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to set permission mode: {err}"));
        }
        Ok(())
    }

    /// Change the permission mode of one session only.
    pub async fn set_session_permission_mode(
        &self,
        session_id: SessionId,
        mode: maukode_core::PermissionMode,
    ) -> Result<()> {
        let resp = self
            .http
            .post(format!("{}/api/v1/permissions/mode", self.base_url))
            .json(&serde_json::json!({ "mode": mode, "session_id": session_id }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Failed to set the session's permission mode: {err}"
            ));
        }
        Ok(())
    }

    pub async fn respond_permission(
        &self,
        session_id: SessionId,
        request_id: &str,
        decision: maukode_protocol::PermissionDecision,
    ) -> Result<()> {
        let resp = self
            .http
            .post(format!(
                "{}/api/v1/sessions/{session_id}/permissions/{request_id}",
                self.base_url
            ))
            .json(&serde_json::json!({ "decision": decision }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to respond to permission request: {err}"));
        }
        Ok(())
    }

    pub async fn get_diff(&self, session_id: SessionId) -> Result<maukode_protocol::DiffResponse> {
        let resp = self
            .http
            .get(format!(
                "{}/api/v1/sessions/{session_id}/diff",
                self.base_url
            ))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to get session diff: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn undo(
        &self,
        session_id: SessionId,
        turns: Option<usize>,
    ) -> Result<maukode_protocol::UndoResponse> {
        let resp = self
            .http
            .post(format!(
                "{}/api/v1/sessions/{session_id}/undo",
                self.base_url
            ))
            .json(&serde_json::json!({ "turns": turns }))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to undo session: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn list_checkpoints(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<maukode_protocol::Checkpoint>> {
        let resp = self
            .http
            .get(format!(
                "{}/api/v1/sessions/{session_id}/checkpoints",
                self.base_url
            ))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list session checkpoints: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_codebase_map(
        &self,
        session_id: SessionId,
        subpath: Option<&str>,
        query: Option<&str>,
        max_tokens: Option<usize>,
    ) -> Result<maukode_protocol::CodebaseMapResponse> {
        let mut req = self.http.get(format!(
            "{}/api/v1/sessions/{session_id}/map",
            self.base_url
        ));

        if let Some(sp) = subpath {
            req = req.query(&[("subpath", sp)]);
        }
        if let Some(q) = query {
            req = req.query(&[("query", q)]);
        }
        if let Some(mt) = max_tokens {
            req = req.query(&[("max_tokens", mt)]);
        }

        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to get codebase map: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn init_project(
        &self,
        session_id: SessionId,
        force: bool,
    ) -> Result<maukode_protocol::InitProjectResponse> {
        let resp = self
            .http
            .post(format!(
                "{}/api/v1/sessions/{session_id}/init",
                self.base_url
            ))
            .json(&maukode_protocol::InitProjectRequest { force })
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to init project: {err}"));
        }
        Ok(resp.json().await?)
    }

    /// Connect the session's MCP servers (spawns them) and report their status.
    pub async fn list_mcp_servers(
        &self,
        session_id: SessionId,
    ) -> Result<maukode_protocol::McpListResponse> {
        let resp = self
            .http
            .post(format!(
                "{}/api/v1/sessions/{session_id}/mcp",
                self.base_url
            ))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list MCP servers: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn compact_session(
        &self,
        session_id: SessionId,
        focus: Option<&str>,
    ) -> Result<maukode_protocol::CompactResponse> {
        let resp = self
            .http
            .post(format!(
                "{}/api/v1/sessions/{session_id}/compact",
                self.base_url
            ))
            .json(&maukode_protocol::CompactRequest {
                focus: focus.map(|s| s.to_string()),
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to compact session: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn list_subagent_profiles(&self) -> Result<Vec<maukode_protocol::SubagentProfile>> {
        let resp = self
            .http
            .get(format!("{}/api/v1/subagents/profiles", self.base_url))
            .send()
            .await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list subagent profiles: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn list_skills(&self, directory: Option<&str>) -> Result<Vec<SkillDefinition>> {
        let mut req = self.http.get(format!("{}/api/v1/skills", self.base_url));
        if let Some(dir) = directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list skills: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_skill(&self, name: &str, directory: Option<&str>) -> Result<SkillDefinition> {
        let mut req = self
            .http
            .get(format!("{}/api/v1/skills/{name}", self.base_url));
        if let Some(dir) = directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to get skill '{name}': {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn list_plugins(&self, directory: Option<&str>) -> Result<Vec<PluginManifest>> {
        let mut req = self.http.get(format!("{}/api/v1/plugins", self.base_url));
        if let Some(dir) = directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to list plugins: {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn get_plugin(&self, name: &str, directory: Option<&str>) -> Result<PluginManifest> {
        let mut req = self
            .http
            .get(format!("{}/api/v1/plugins/{name}", self.base_url));
        if let Some(dir) = directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to get plugin '{name}': {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn enable_plugin(
        &self,
        name: &str,
        directory: Option<&str>,
    ) -> Result<PluginManifest> {
        let mut req = self
            .http
            .post(format!("{}/api/v1/plugins/{name}/enable", self.base_url));
        if let Some(dir) = directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to enable plugin '{name}': {err}"));
        }
        Ok(resp.json().await?)
    }

    pub async fn disable_plugin(
        &self,
        name: &str,
        directory: Option<&str>,
    ) -> Result<PluginManifest> {
        let mut req = self
            .http
            .post(format!("{}/api/v1/plugins/{name}/disable", self.base_url));
        if let Some(dir) = directory {
            req = req.query(&[("directory", dir)]);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let err = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Failed to disable plugin '{name}': {err}"));
        }
        Ok(resp.json().await?)
    }
}
