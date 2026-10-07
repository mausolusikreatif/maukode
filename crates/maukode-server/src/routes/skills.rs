use crate::state::ServerState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Deserialize)]
pub struct SkillsQuery {
    pub directory: Option<String>,
}

pub async fn list_skills(
    State(state): State<Arc<ServerState>>,
    Query(query): Query<SkillsQuery>,
) -> impl IntoResponse {
    let dir = query
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());
    let skills = state.skill_manager.list_skills(Some(&dir));
    Json(skills)
}

pub async fn get_skill(
    State(state): State<Arc<ServerState>>,
    Path(name): Path<String>,
    Query(query): Query<SkillsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    let dir = query
        .directory
        .map(PathBuf::from)
        .unwrap_or_else(|| state.default_dir.clone());
    state
        .skill_manager
        .get_skill(&name, Some(&dir))
        .map(Json)
        .ok_or_else(|| (StatusCode::NOT_FOUND, format!("Skill '{name}' not found")))
}
