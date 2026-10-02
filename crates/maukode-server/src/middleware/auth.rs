use crate::state::ServerState;
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::sync::Arc;

pub async fn auth_middleware(
    State(state): State<Arc<ServerState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(ref expected_token) = state.auth_token {
        let auth_header = req
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|val| val.to_str().ok());

        let is_valid = match auth_header {
            Some(header) => {
                if let Some(token) = header.strip_prefix("Bearer ") {
                    token.trim() == expected_token.trim()
                } else {
                    false
                }
            }
            None => false,
        };

        if !is_valid {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    Ok(next.run(req).await)
}
