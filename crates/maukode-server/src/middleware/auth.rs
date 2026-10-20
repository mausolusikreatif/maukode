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

        let is_valid = auth_header
            .and_then(|header| header.strip_prefix("Bearer "))
            .is_some_and(|token| constant_time_eq(token.trim(), expected_token.trim()));

        if !is_valid {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    Ok(next.run(req).await)
}

/// Compare two secrets in time that depends only on their lengths, not on where they differ, so
/// response timing does not reveal how much of a guessed token is right.
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn constant_time_eq_compares_whole_tokens() {
        assert!(constant_time_eq("secret-token", "secret-token"));
        assert!(!constant_time_eq("secret-token", "secret-tokem"));
        assert!(!constant_time_eq("secret", "secret-token"));
        assert!(!constant_time_eq("", "x"));
        assert!(constant_time_eq("", ""));
    }
}
