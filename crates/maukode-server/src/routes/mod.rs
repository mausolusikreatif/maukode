pub mod health;
pub mod providers;
pub mod sessions;

use crate::middleware::auth::auth_middleware;
use crate::state::ServerState;
use axum::{
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::{from_fn, from_fn_with_state, Next},
    response::Response,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::trace::TraceLayer;

/// Origins that may talk to a token-less (local-only) server: loopback pages and desktop shells.
fn is_local_origin(origin: &str) -> bool {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
        .or_else(|| origin.strip_prefix("tauri://"));
    let Some(rest) = rest else { return false };
    let host = rest.split(':').next().unwrap_or(rest);
    matches!(
        host,
        "localhost" | "127.0.0.1" | "[::1]" | "tauri.localhost"
    )
}

/// Without a bearer token, any web page could drive the local agent (CSRF). Reject foreign
/// browser origins; non-browser clients send no `Origin` and pass.
async fn reject_foreign_origin(req: Request, next: Next) -> Result<Response, StatusCode> {
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        if !origin.to_str().is_ok_and(is_local_origin) {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    Ok(next.run(req).await)
}

pub fn create_router(state: Arc<ServerState>) -> Router {
    let cors = CorsLayer::new().allow_methods(Any).allow_headers(Any);
    let guarded = state.auth_token.is_none();
    let cors = if guarded {
        cors.allow_origin(AllowOrigin::predicate(|origin: &HeaderValue, _| {
            origin.to_str().is_ok_and(is_local_origin)
        }))
    } else {
        cors.allow_origin(Any)
    };

    // Public / health routes
    let public_routes = Router::new()
        .route("/health", get(health::health_check))
        .route("/api/v1/health", get(health::health_check));

    // Protected API routes
    let api_routes = Router::new()
        // Providers & models
        .route(
            "/api/v1/providers",
            get(providers::list_providers).post(providers::add_or_update_provider),
        )
        .route(
            "/api/v1/providers/active",
            post(providers::set_active_provider),
        )
        .route(
            "/api/v1/config/agent",
            get(providers::get_agent_config).post(providers::set_agent_config),
        )
        .route("/api/v1/models", get(providers::list_models))
        .route("/api/v1/models/active", post(providers::set_active_model))
        .route(
            "/api/v1/permissions/mode",
            get(sessions::get_permission_mode).post(sessions::set_permission_mode),
        )
        // Sessions
        .route(
            "/api/v1/sessions",
            get(sessions::list_sessions).post(sessions::create_session),
        )
        .route(
            "/api/v1/sessions/{id}",
            get(sessions::get_session)
                .patch(sessions::update_session)
                .delete(sessions::delete_session),
        )
        .route(
            "/api/v1/sessions/{id}/audit",
            get(sessions::get_session_audit),
        )
        .route(
            "/api/v1/sessions/{id}/prompt",
            post(sessions::submit_prompt),
        )
        .route(
            "/api/v1/sessions/{id}/cancel",
            post(sessions::cancel_prompt),
        )
        .route(
            "/api/v1/sessions/{id}/events",
            get(sessions::stream_session_events),
        )
        .route(
            "/api/v1/sessions/{id}/permissions/{request_id}",
            post(sessions::respond_permission),
        )
        .route(
            "/api/v1/sessions/{id}/diff",
            get(sessions::get_session_diff),
        )
        .route("/api/v1/sessions/{id}/undo", post(sessions::undo_session))
        .route(
            "/api/v1/sessions/{id}/checkpoints",
            get(sessions::list_session_checkpoints),
        )
        .route("/api/v1/sessions/{id}/map", get(sessions::get_codebase_map))
        .route("/api/v1/sessions/{id}/init", post(sessions::init_project))
        .route("/api/v1/sessions/{id}/mcp", get(sessions::list_mcp_servers))
        .route(
            "/api/v1/sessions/{id}/compact",
            post(sessions::compact_session),
        )
        .route_layer(from_fn_with_state(Arc::clone(&state), auth_middleware));

    let router = Router::new().merge(public_routes).merge(api_routes);
    let router = if guarded {
        router.layer(from_fn(reject_foreign_origin))
    } else {
        router
    };
    router
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
