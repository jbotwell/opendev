//! MCP (Model Context Protocol) server management routes.
//!
//! These endpoints manage MCP server configurations. Actual MCP server
//! connections are not yet implemented in the Rust port — these routes
//! provide the API surface that the frontend expects, backed by JSON
//! config file persistence.

mod io;
mod models;

use std::path::PathBuf;

use axum::extract::{Path as AxumPath, State};
use axum::routing::{get, post};
use axum::{Json, Router};

pub use models::{McpServerConfig, McpServerCreate, McpServerUpdate};

use crate::error::WebError;
use crate::state::{AppState, WsBroadcast};

use io::{
    global_config_path, load_all_servers, project_config_path, remove_server_from_config,
    save_server_to_config,
};

/// Build the MCP router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/mcp/servers", get(list_servers).post(create_server))
        .route(
            "/api/mcp/servers/{name}",
            get(get_server).put(update_server).delete(delete_server),
        )
        .route("/api/mcp/servers/{name}/connect", post(connect_server))
        .route(
            "/api/mcp/servers/{name}/disconnect",
            post(disconnect_server),
        )
}

/// List all configured MCP servers.
async fn list_servers(State(state): State<AppState>) -> Result<Json<serde_json::Value>, WebError> {
    let servers = load_all_servers(state.working_dir());

    let result: Vec<serde_json::Value> = servers
        .iter()
        .map(|(name, config)| {
            serde_json::json!({
                "name": name,
                "status": "disconnected",
                "config": {
                    "command": config.command,
                    "args": config.args,
                    "env": config.env,
                    "enabled": config.enabled,
                    "auto_start": config.auto_start,
                },
                "tools_count": 0,
                "config_location": "global",
                "config_path": global_config_path().to_string_lossy(),
            })
        })
        .collect();

    Ok(Json(serde_json::json!({"servers": result})))
}

/// Get details about a specific MCP server.
async fn get_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, WebError> {
    let servers = load_all_servers(state.working_dir());
    let config = servers
        .get(&name)
        .ok_or_else(|| WebError::NotFound(format!("Server '{}' not found", name)))?;

    Ok(Json(serde_json::json!({
        "name": name,
        "status": "disconnected",
        "config": {
            "command": config.command,
            "args": config.args,
            "env": config.env,
            "enabled": config.enabled,
            "auto_start": config.auto_start,
        },
        "tools": [],
        "capabilities": [],
        "config_path": global_config_path().to_string_lossy(),
    })))
}

/// Create a new MCP server.
async fn create_server(
    State(state): State<AppState>,
    Json(payload): Json<McpServerCreate>,
) -> Result<Json<serde_json::Value>, WebError> {
    let servers = load_all_servers(state.working_dir());
    if servers.contains_key(&payload.name) {
        return Err(WebError::BadRequest(format!(
            "Server '{}' already exists",
            payload.name
        )));
    }

    let config = McpServerConfig {
        command: payload.command,
        args: payload.args,
        env: payload.env,
        enabled: payload.enabled,
        auto_start: payload.auto_start,
    };

    save_server_to_config(&payload.name, &config, &global_config_path())?;

    state.broadcast(WsBroadcast {
        msg_type: "mcp_servers_updated".to_string(),
        data: serde_json::json!({
            "action": "added",
            "server_name": payload.name,
        }),
    });

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Server '{}' added successfully", payload.name),
    })))
}

/// Update an existing MCP server.
async fn update_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    Json(update): Json<McpServerUpdate>,
) -> Result<Json<serde_json::Value>, WebError> {
    let servers = load_all_servers(state.working_dir());
    let existing = servers
        .get(&name)
        .ok_or_else(|| WebError::NotFound(format!("Server '{}' not found", name)))?;

    let config = McpServerConfig {
        command: update.command.unwrap_or_else(|| existing.command.clone()),
        args: update.args.unwrap_or_else(|| existing.args.clone()),
        env: update.env.unwrap_or_else(|| existing.env.clone()),
        enabled: update.enabled.unwrap_or(existing.enabled),
        auto_start: update.auto_start.unwrap_or(existing.auto_start),
    };

    save_server_to_config(&name, &config, &global_config_path())?;

    state.broadcast(WsBroadcast {
        msg_type: "mcp_servers_updated".to_string(),
        data: serde_json::json!({
            "action": "updated",
            "server_name": name,
        }),
    });

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Server '{}' updated successfully", name),
    })))
}

/// Delete an MCP server.
async fn delete_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, WebError> {
    let servers = load_all_servers(state.working_dir());
    if !servers.contains_key(&name) {
        return Err(WebError::NotFound(format!("Server '{}' not found", name)));
    }

    // Try to remove from both global and project configs.
    let global_removed = remove_server_from_config(&name, &global_config_path())?;
    let project_removed =
        remove_server_from_config(&name, &project_config_path(state.working_dir()))?;

    if !global_removed && !project_removed {
        return Err(WebError::Internal(format!(
            "Server '{}' found in memory but not in config files",
            name
        )));
    }

    state.broadcast(WsBroadcast {
        msg_type: "mcp_servers_updated".to_string(),
        data: serde_json::json!({
            "action": "removed",
            "server_name": name,
        }),
    });

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Server '{}' removed successfully", name),
    })))
}

/// Connect to an MCP server.
///
/// Loads the server configuration, creates a transport via `opendev_mcp`,
/// runs the MCP initialize handshake, and discovers available tools.
async fn connect_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, WebError> {
    let servers = load_all_servers(state.working_dir());
    let server_config = servers
        .get(&name)
        .ok_or_else(|| WebError::NotFound(format!("Server '{}' not found", name)))?;

    // Build an opendev_mcp::McpServerConfig from the web-layer config.
    let mcp_config = opendev_mcp::McpServerConfig {
        command: server_config.command.clone(),
        args: server_config.args.clone(),
        env: server_config.env.clone(),
        enabled: server_config.enabled,
        auto_start: server_config.auto_start,
        ..Default::default()
    };

    // Use McpManager to connect. It handles transport creation, the
    // initialize handshake, and tool discovery in one call.
    let manager = opendev_mcp::McpManager::new(Some(PathBuf::from(state.working_dir())));
    manager
        .add_server(name.clone(), mcp_config)
        .await
        .map_err(|e| WebError::Internal(format!("Failed to register server: {}", e)))?;

    manager.connect_server(&name).await.map_err(|e| {
        WebError::Internal(format!("Failed to connect to MCP server '{}': {}", name, e))
    })?;

    // Count the tools discovered during the connection.
    let schemas = manager.get_all_tool_schemas().await;
    let tools_count = schemas.len();

    // Disconnect the manager-owned transport — the web layer does not hold
    // long-lived connections yet; this endpoint proves connectivity and
    // reports the tool count.
    let _ = manager.disconnect_server(&name).await;

    state.broadcast(WsBroadcast {
        msg_type: "mcp_servers_updated".to_string(),
        data: serde_json::json!({
            "action": "connected",
            "server_name": &name,
            "tools_count": tools_count,
        }),
    });

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Connected to '{}' — {} tool(s) discovered", name, tools_count),
        "tools_count": tools_count,
    })))
}

/// Disconnect from an MCP server.
///
/// Connection logic is not yet implemented in the Rust port.
/// Returns a placeholder response.
async fn disconnect_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
) -> Result<Json<serde_json::Value>, WebError> {
    let servers = load_all_servers(state.working_dir());
    if !servers.contains_key(&name) {
        return Err(WebError::NotFound(format!("Server '{}' not found", name)));
    }

    Ok(Json(serde_json::json!({
        "success": true,
        "message": format!("Not connected to '{}'", name),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use opendev_history::SessionManager;
    use opendev_models::AppConfig;
    use tempfile::TempDir;
    use tower::ServiceExt;

    fn make_state_with_workdir(work_dir: &str) -> AppState {
        let tmp = TempDir::new().unwrap();
        let tmp_path = tmp.into_path();
        let session_manager = SessionManager::new(tmp_path.clone()).unwrap();
        let config = AppConfig::default();
        let user_store = opendev_http::UserStore::new(tmp_path).unwrap();
        let model_registry = opendev_config::ModelRegistry::new();
        AppState::new(
            session_manager,
            config,
            work_dir.to_string(),
            user_store,
            model_registry,
        )
    }

    #[tokio::test]
    async fn test_list_servers_empty() {
        let tmp = TempDir::new().unwrap();
        let state = make_state_with_workdir(&tmp.path().to_string_lossy());

        let app = crate::server::build_app(state, None);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/mcp/servers")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        // May or may not have servers depending on user's ~/.opendev/mcp.json
        assert!(json["servers"].is_array());
    }

    #[tokio::test]
    async fn test_get_server_not_found() {
        let tmp = TempDir::new().unwrap();
        let state = make_state_with_workdir(&tmp.path().to_string_lossy());

        let app = crate::server::build_app(state, None);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/mcp/servers/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_create_and_get_server() {
        let tmp = TempDir::new().unwrap();
        // Use a temp dir as working dir so we don't write to real ~/.opendev/
        let work_dir = tmp.path().to_string_lossy().to_string();

        // Override HOME to the temp dir so global_config_path resolves there.
        // SAFETY: test-only; overrides HOME so config resolves to temp dir.
        unsafe { std::env::set_var("HOME", tmp.path()) };

        let state = make_state_with_workdir(&work_dir);

        // Create the .opendev directory.
        std::fs::create_dir_all(tmp.path().join(".opendev")).unwrap();

        // Create server.
        let app = crate::server::build_app(state.clone(), None);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/mcp/servers")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"name":"test-server","command":"uvx","args":["mcp-server-test"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        // Get server.
        let app = crate::server::build_app(state.clone(), None);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/mcp/servers/test-server")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["name"], "test-server");
        assert_eq!(json["config"]["command"], "uvx");

        // Delete server.
        let app = crate::server::build_app(state.clone(), None);
        let response = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri("/api/mcp/servers/test-server")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_connect_server_not_found() {
        let tmp = TempDir::new().unwrap();
        let state = make_state_with_workdir(&tmp.path().to_string_lossy());

        let app = crate::server::build_app(state, None);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/mcp/servers/nonexistent/connect")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
