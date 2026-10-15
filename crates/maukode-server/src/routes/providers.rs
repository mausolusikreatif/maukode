use crate::state::ServerState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use maukode_core::{ProviderConfig, ProviderKind};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetActiveProviderRequest {
    pub provider_id: String,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetActiveModelRequest {
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct GetModelsQuery {
    pub provider: Option<String>,
}

/// A provider as shown to clients: stored secrets are never included.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ProviderView {
    pub id: String,
    pub name: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub has_api_key: bool,
    pub default_model: Option<String>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ProvidersListResponse {
    pub active_provider: String,
    pub active_model: String,
    pub providers: Vec<ProviderView>,
}

#[derive(Debug, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ModelsListResponse {
    pub provider: String,
    pub models: Vec<String>,
}

pub async fn list_providers(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let config = state.config.read().await;
    Json(ProvidersListResponse {
        active_provider: config.active_provider.clone(),
        active_model: config.active_model.clone(),
        providers: config
            .providers
            .iter()
            .map(|p| ProviderView {
                id: p.id.clone(),
                name: p.name.clone(),
                kind: p.kind.clone(),
                base_url: p.base_url.clone(),
                has_api_key: p.api_key.is_some(),
                default_model: p.default_model.clone(),
            })
            .collect(),
    })
}

pub async fn add_or_update_provider(
    State(state): State<Arc<ServerState>>,
    Json(mut payload): Json<ProviderConfig>,
) -> impl IntoResponse {
    let mut config = state.config.write().await;
    // Clients receive redacted keys, so a missing key means "keep the stored one" and an empty
    // string means "remove it".
    match payload.api_key.as_deref() {
        None => {
            payload.api_key = config
                .get_provider(&payload.id)
                .and_then(|p| p.api_key.clone());
        }
        Some("") => payload.api_key = None,
        Some(_) => {}
    }
    config.add_or_update_provider(payload);
    Json(json!({ "status": "ok" }))
}

pub async fn set_active_provider(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<SetActiveProviderRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut config = state.config.write().await;
    config
        .set_active_provider(&payload.provider_id)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;

    Ok(Json(json!({
        "status": "ok",
        "active_provider": config.active_provider,
        "active_model": config.active_model,
    })))
}

pub async fn list_models(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<GetModelsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let config = state.config.read().await;
    let target_provider = query
        .provider
        .unwrap_or_else(|| config.active_provider.clone());

    let models = config
        .fetch_models_for_provider(&target_provider)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(ModelsListResponse {
        provider: target_provider,
        models,
    }))
}

pub async fn set_active_model(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<SetActiveModelRequest>,
) -> impl IntoResponse {
    let mut config = state.config.write().await;
    config.set_active_model(&payload.model);
    Json(json!({
        "status": "ok",
        "active_model": config.active_model,
    }))
}

#[derive(Debug, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct SetAgentConfigRequest {
    pub max_steps: Option<usize>,
    pub max_tokens: Option<u32>,
    pub reflection: Option<maukode_core::ReflectionConfig>,
    pub consolidation: Option<maukode_core::ConsolidationConfig>,
}

pub async fn get_agent_config(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    Json(state.config.read().await.agent.clone())
}

pub async fn set_agent_config(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<SetAgentConfigRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut config = state.config.write().await;
    let mut agent = config.agent.clone();
    if let Some(steps) = payload.max_steps {
        if !(1..=10_000).contains(&steps) {
            return Err((
                StatusCode::BAD_REQUEST,
                "max_steps must be 1..=10000".into(),
            ));
        }
        agent.max_steps = steps;
    }
    if let Some(tokens) = payload.max_tokens {
        if !(1..=200_000).contains(&tokens) {
            return Err((
                StatusCode::BAD_REQUEST,
                "max_tokens must be 1..=200000".into(),
            ));
        }
        agent.max_tokens = tokens;
    }
    if let Some(reflection) = payload.reflection {
        agent.reflection = reflection;
    }
    if let Some(consolidation) = payload.consolidation {
        agent.consolidation = consolidation;
    }
    config.agent = agent.clone();
    config
        .save()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(agent))
}

/// Remove a provider (never the active one). Sessions pinned to it fall back to the defaults.
pub async fn delete_provider(
    State(state): State<Arc<ServerState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut config = state.config.write().await;
    match config.remove_provider(&id) {
        Ok(true) => Ok(Json(json!({ "status": "removed" }))),
        Ok(false) => Err((StatusCode::NOT_FOUND, format!("Provider '{id}' not found"))),
        Err(e) => Err((StatusCode::CONFLICT, e.to_string())),
    }
}
