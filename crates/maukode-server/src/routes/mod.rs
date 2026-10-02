pub mod health;
pub mod providers;
pub mod sessions;

use std::sync::Arc;
use axum::{
    middleware::from_fn_with_state,
    routing::{get, post},
    Router,
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use crate::middleware::auth::auth_middleware;
use crate::state::ServerState;

pub fn create_router(state: Arc<ServerState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

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
        .route("/api/v1/models", get(providers::list_models))
        .route("/api/v1/models/active", post(providers::set_active_model))
        // Sessions
        .route(
            "/api/v1/sessions",
            get(sessions::list_sessions).post(sessions::create_session),
        )
        .route("/api/v1/sessions/{id}", get(sessions::get_session))
        .route(
            "/api/v1/sessions/{id}/prompt",
            post(sessions::submit_prompt),
        )
        .route(
            "/api/v1/sessions/{id}/events",
            get(sessions::stream_session_events),
        )
        .route(
            "/api/v1/sessions/{id}/permissions/{request_id}",
            post(sessions::respond_permission),
        )
        .route_layer(from_fn_with_state(Arc::clone(&state), auth_middleware));

    Router::new()
        .merge(public_routes)
        .merge(api_routes)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
