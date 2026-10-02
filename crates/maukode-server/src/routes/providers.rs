use std::sync::Arc;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use maukode_core::ProviderConfig;
use serde::Deserialize;
use serde_json::json;
use crate::state::ServerState;

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
    Json(json!({
        "active_provider": config.active_provider,
        "active_model": config.active_model,
        "providers": config.providers,
    }))
}

pub async fn add_or_update_provider(
    State(state): State<Arc<ServerState>>,
    Json(payload): Json<ProviderConfig>,
) -> impl IntoResponse {
    let mut config = state.config.write().await;
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
