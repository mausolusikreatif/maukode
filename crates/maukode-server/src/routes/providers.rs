use crate::state::ServerState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use maukode_core::ProviderConfig;
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct SetActiveProviderRequest {
    pub provider_id: String,
}

#[derive(Debug, Deserialize)]
pub struct SetActiveModelRequest {
    pub model: String,
}

#[derive(Debug, Deserialize)]
pub struct GetModelsQuery {
    pub provider: Option<String>,
}

pub async fn list_providers(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    let config = state.config.read().await;
    // Never ship stored secrets to clients; expose only whether a key is set.
    let providers: Vec<_> = config
        .providers
        .iter()
        .map(|p| {
            let mut value = serde_json::to_value(p).unwrap_or_default();
            if let Some(obj) = value.as_object_mut() {
                obj.insert("has_api_key".into(), json!(p.api_key.is_some()));
                obj.insert("api_key".into(), serde_json::Value::Null);
            }
            value
        })
        .collect();
    Json(json!({
        "active_provider": config.active_provider,
        "active_model": config.active_model,
        "providers": providers,
    }))
}

pub async fn add_or_update_provider(
    State(state): State<Arc<ServerState>>,
    Json(mut payload): Json<ProviderConfig>,
) -> impl IntoResponse {
    let mut config = state.config.write().await;
    // Clients receive redacted keys, so a missing key means "keep the stored one".
    if payload.api_key.is_none() {
        payload.api_key = config
            .get_provider(&payload.id)
            .and_then(|p| p.api_key.clone());
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

    Ok(Json(json!({
        "provider": target_provider,
        "models": models,
    })))
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
pub struct SetAgentConfigRequest {
    pub max_steps: Option<usize>,
    pub max_tokens: Option<u32>,
}

pub async fn get_agent_config(State(state): State<Arc<ServerState>>) -> impl IntoResponse {
    Json(state.config.read().await.agent)
}

pub async fn set_agent_config(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<SetAgentConfigRequest>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let mut config = state.config.write().await;
    let mut agent = config.agent;
    if let Some(steps) = payload.max_steps {
        if !(1..=200).contains(&steps) {
            return Err((StatusCode::BAD_REQUEST, "max_steps must be 1..=200".into()));
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
    config.agent = agent;
    config
        .save()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(agent))
}
