pub mod auth;
pub mod events;
pub mod health;
pub mod plugins;
pub mod preview;
pub mod providers;
pub mod sessions;
pub mod settings;
pub mod skills;

use crate::middleware::auth::auth_middleware;
use crate::state::ServerState;
use axum::{
    extract::{Request, State},
    http::{header, HeaderName, HeaderValue, StatusCode},
    middleware::{from_fn_with_state, Next},
    response::Response,
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

/// Hostname part of an authority (`host`, `host:port`, `[v6]:port`), lowercased, without a
/// trailing root dot. Hostnames are case-insensitive and `localhost.` is the same name.
fn authority_host(authority: &str) -> String {
    let authority = authority.split('/').next().unwrap_or_default();
    let host = if let Some(rest) = authority.strip_prefix('[') {
        let inner = rest.split(']').next().unwrap_or_default();
        format!("[{inner}]")
    } else {
        authority.split(':').next().unwrap_or_default().to_string()
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

fn is_loopback_name(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Origins that may talk to a token-less (local-only) server: pages served from loopback.
fn is_local_origin(origin: &str) -> bool {
    let rest = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"));
    rest.is_some_and(|r| is_loopback_name(&authority_host(r)))
}

/// Host names a local client legitimately uses to reach a loopback server.
fn is_local_host(host_header: &str) -> bool {
    is_loopback_name(&authority_host(host_header))
}

/// Without a bearer token, any web page could talk to the local agent. Two attacks, two checks:
/// - CSRF / cross-origin reads: reject foreign `Origin`s (non-browser clients send none).
/// - DNS rebinding: a page on `evil.example` re-resolved to 127.0.0.1 is *same-origin* with the
///   daemon, so browsers attach no `Origin` to its GET requests and it could read every session,
///   diff and conversation. Its `Host` header still says `evil.example`, so reject that too.
///
/// The Host check only applies to servers bound to loopback; on other addresses clients
/// legitimately use other names, and that is the operator's choice (use a token there).
async fn reject_foreign_origin(
    State(state): State<Arc<ServerState>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        if !origin.to_str().is_ok_and(is_local_origin) {
            return Err(StatusCode::FORBIDDEN);
        }
    }
    if state.enforce_local_host {
        if let Some(host) = req.headers().get(header::HOST) {
            if !host.to_str().is_ok_and(is_local_host) {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }
    Ok(next.run(req).await)
}

pub fn create_router(state: Arc<ServerState>) -> Router {
    let cors = CorsLayer::new()
        .allow_methods(Any)
        .allow_headers(Any)
        .expose_headers([HeaderName::from_static("x-event-seq")]);
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
        // Subagents
        .route(
            "/api/v1/subagents/profiles",
            get(sessions::list_subagent_profiles),
        )
        // Skills
        .route("/api/v1/skills", get(skills::list_skills))
        .route("/api/v1/skills/{name}", get(skills::get_skill))
        // Plugins
        .route("/api/v1/plugins", get(plugins::list_plugins))
        .route("/api/v1/plugins/{name}", get(plugins::get_plugin))
        .route(
            "/api/v1/plugins/{name}/enable",
            post(plugins::enable_plugin),
        )
        .route(
            "/api/v1/plugins/{name}/disable",
            post(plugins::disable_plugin),
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
            "/api/v1/sessions/{id}/queue",
            get(sessions::get_session_queue)
                .post(sessions::queue_prompt)
                .delete(sessions::clear_session_queue),
        )
        .route("/api/v1/sessions/{id}/steer", post(sessions::steer_prompt))
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
        .route(
            "/api/v1/sessions/{id}/diff/files",
            get(sessions::get_session_diff_files),
        )
        .route(
            "/api/v1/sessions/{id}/diff/file",
            get(sessions::get_session_file_diff),
        )
        .route("/api/v1/sessions/{id}/undo", post(sessions::undo_session))
        .route(
            "/api/v1/sessions/{id}/messages",
            get(sessions::get_session_messages),
        )
        .route(
            "/api/v1/sessions/{id}/preview",
            post(preview::start_preview)
                .get(preview::get_preview)
                .delete(preview::stop_preview),
        )
        .route(
            "/api/v1/sessions/{id}/preview/proxy",
            axum::routing::any(preview::proxy_preview_root),
        )
        .route(
            "/api/v1/sessions/{id}/preview/proxy/",
            axum::routing::any(preview::proxy_preview_root),
        )
        .route(
            "/api/v1/sessions/{id}/preview/proxy/{*rest}",
            axum::routing::any(preview::proxy_preview_path),
        )
        .route(
            "/api/v1/sessions/{id}/model",
            axum::routing::put(sessions::set_session_model),
        )
        .route(
            "/api/v1/sessions/{id}/worktree",
            get(sessions::get_worktree),
        )
        .route(
            "/api/v1/sessions/{id}/worktree/commit",
            post(sessions::commit_worktree),
        )
        .route(
            "/api/v1/sessions/{id}/worktree/merge",
            post(sessions::merge_worktree),
        )
        .route("/api/v1/events", get(events::global_events))
        .route(
            "/api/v1/providers/{id}",
            axum::routing::delete(providers::delete_provider),
        )
        .route(
            "/api/v1/config/security",
            get(settings::get_security).put(settings::set_default_permission_mode),
        )
        .route(
            "/api/v1/config/security/sandbox",
            axum::routing::put(settings::set_sandbox_mode),
        )
        .route(
            "/api/v1/config/execution",
            get(settings::get_execution).put(settings::set_execution),
        )
        .route(
            "/api/v1/config/execution/status",
            get(settings::execution_status),
        )
        .route("/api/v1/mcp", get(settings::list_mcp))
        .route(
            "/api/v1/auth/chatgpt",
            get(auth::status).delete(auth::logout),
        )
        .route("/api/v1/auth/chatgpt/login", post(auth::begin_login))
        .route(
            "/api/v1/auth/chatgpt/login/{attempt_id}",
            get(auth::login_status).delete(auth::cancel_login),
        )
        .route("/api/v1/auth/chatgpt/account", post(auth::use_account))
        .route(
            "/api/v1/sessions/{id}/checkpoints",
            get(sessions::list_session_checkpoints),
        )
        .route("/api/v1/sessions/{id}/map", get(sessions::get_codebase_map))
        .route("/api/v1/sessions/{id}/init", post(sessions::init_project))
        .route(
            "/api/v1/sessions/{id}/mcp",
            get(sessions::list_mcp_servers).post(sessions::connect_mcp_servers),
        )
        .route(
            "/api/v1/sessions/{id}/compact",
            post(sessions::compact_session),
        )
        .route_layer(from_fn_with_state(Arc::clone(&state), auth_middleware));

    let router = Router::new().merge(public_routes).merge(api_routes);
    let router = if guarded {
        router.layer(from_fn_with_state(
            Arc::clone(&state),
            reject_foreign_origin,
        ))
    } else {
        router
    };

    // If a web UI directory is configured, serve its static files as a SPA
    // fallback. Any path not matched by the API routes above falls through to
    // the ServeDir, which serves assets from the directory and falls back to
    // `index.html` for unknown paths (client-side routing).
    let router = if let Some(web_dir) = &state.web_dir {
        let serve_dir = ServeDir::new(web_dir).fallback(ServeFile::new(web_dir.join("index.html")));
        router.fallback_service(serve_dir)
    } else {
        router
    };

    router
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
