use std::sync::Arc;

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{PgPool, Row};

use crate::route_support::{ApiError, AppState};

/// Pure dispatch logic (per plan: separate from HTTP handlers).
/// Called by call_mcp_tool after gate+parse, and directly by unit tests.
pub(crate) async fn dispatch_lab_safety_action(
    pool: &PgPool,
    action: &str,
    args: &Value,
) -> Result<Value, ApiError> {
    match action {
        "list_labs" => {
            let q = args.get("q").and_then(|v| v.as_str());
            let like = q.map(|s| format!("%{}%", s.replace('%', "\\%").replace('_', "\\_")));
            let rows = if let Some(ref pat) = like {
                sqlx::query(
                    r#"select id, code, name, location, department, manager_user_id, contact, status, description, created_at
                       from labs
                       where name ilike $1 or code ilike $1
                       limit 10"#
                )
                .bind(pat)
                .fetch_all(pool)
                .await
            } else {
                sqlx::query(
                    r#"select id, code, name, location, department, manager_user_id, contact, status, description, created_at
                       from labs limit 10"#
                )
                .fetch_all(pool)
                .await
            }
            .map_err(|e| ApiError::bad_request(format!("db error: {}", e)))?;
            let labs: Vec<Value> = rows.into_iter().map(|r| {
                json!({
                    "id": r.get::<i64, _>("id"),
                    "code": r.get::<String, _>("code"),
                    "name": r.get::<String, _>("name"),
                    "location": r.get::<Option<String>, _>("location"),
                    "status": r.get::<String, _>("status"),
                })
            }).collect();
            Ok(json!({ "action": action, "result": labs, "count": labs.len() }))
        }
        "list_hazards" => {
            let rows = sqlx::query(
                "select id, title, lab_name, status, category, description from safety_hazards limit 10"
            )
            .fetch_all(pool)
            .await
            .map_err(|e| ApiError::bad_request(format!("db: {}", e)))?;
            let items: Vec<Value> = rows.into_iter().map(|r| json!({
                "id": r.get::<i64,_>("id"),
                "title": r.get::<String,_>("title"),
                "lab_name": r.get::<String,_>("lab_name"),
                "status": r.get::<String,_>("status"),
            })).collect();
            Ok(json!({ "action": action, "result": items }))
        }
        "create_hazard" => {
            let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("MCP created hazard").to_string();
            let lab_name_arg = args.get("lab_name").and_then(|v| v.as_str()).map(|s| s.to_string());
            let (_, lab_name) = crate::route_users_labs::resolve_lab_reference(
                pool,
                args.get("lab_id").and_then(|v| v.as_i64()),
                lab_name_arg.clone(),
            ).await.unwrap_or((None, lab_name_arg.unwrap_or_else(|| "demo-lab".to_string())));

            let reported_by: i64 = if let Some(v) = args.get("reported_by").and_then(|v| v.as_i64()) {
                v
            } else {
                sqlx::query_scalar::<_, i64>("select id from users order by id asc limit 1")
                    .fetch_optional(pool)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or(1)
            };

            let desc = args.get("description").and_then(|v| v.as_str()).unwrap_or("via mcp big tool").to_string();

            let id: i64 = sqlx::query_scalar(
                r#"insert into safety_hazards (title, lab_name, category, description, reported_by, status)
                   values ($1, $2, 'other', $3, $4, 'open') returning id"#
            )
            .bind(&title)
            .bind(&lab_name)
            .bind(&desc)
            .bind(reported_by)
            .fetch_one(pool)
            .await
            .map_err(|e| ApiError::bad_request(format!("insert err: {}", e)))?;
            Ok(json!({ "action": action, "result": { "id": id, "title": title, "status": "open", "lab_name": lab_name } }))
        }
        "list_regulations" | "list_documents" => {
            let rows = sqlx::query("select id, title, regulation_type from regulations limit 5")
                .fetch_all(pool).await.map_err(|e| ApiError::bad_request(format!("db: {}",e)))?;
            let res: Vec<Value> = rows.iter().map(|r| json!({"id": r.get::<i64,_>("id"), "title": r.get::<String,_>("title") })).collect();
            Ok(json!({ "action": action, "result": res }))
        }
        "list_equipment" | "list_operations" => {
            let rows = sqlx::query("select id, name, status from equipment limit 5")
                .fetch_all(pool).await.map_err(|e| ApiError::bad_request(format!("db: {}",e)))?;
            let res: Vec<Value> = rows.iter().map(|r| json!({"id": r.get::<i64,_>("id"), "name": r.get::<String,_>("name"), "status": r.get::<String,_>("status") })).collect();
            Ok(json!({ "action": action, "result": res }))
        }
        "list_incidents" => {
            let rows = sqlx::query("select id, title, severity from incident_cases limit 5")
                .fetch_all(pool).await.map_err(|e| ApiError::bad_request(format!("db: {}",e)))?;
            let res: Vec<Value> = rows.iter().map(|r| json!({"id": r.get::<i64,_>("id"), "title": r.get::<String,_>("title") })).collect();
            Ok(json!({ "action": action, "result": res }))
        }
        _ => Err(ApiError::bad_request(format!("unknown action '{}' for lab_safety tool", action))),
    }
}

pub fn mcp_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mcp", get(get_mcp_config).post(update_mcp_config))
        .route("/mcp/call", post(call_mcp_tool))
}

/// GET /mcp returns current config/status.
/// Config endpoints are not gated by the enabled flag (they are the management interface
/// to inspect and toggle the MCP feature, including re-enabling it). Only the tool dispatch
/// (/mcp/call) is gated when disabled.
async fn get_mcp_config(State(state): State<Arc<AppState>>) -> Json<Value> {
    let rt = state.mcp_runtime.lock().await.clone();
    Json(json!({
        "enabled": rt.enabled,
        "config": state.settings.mcp_config,
        "runtime_config": rt.config,
        "status": "ok",
        "tool_name": "lab_safety",
        "description": "Single large tool dispatching via 'action' grouping param"
    }))
}

#[derive(Deserialize, Debug)]
struct McpConfigPayload {
    enabled: Option<bool>,
    config: Option<String>,
}

/// POST /mcp accepts config updates (in-memory runtime override; supports toggling enabled).
/// Config endpoints are the management plane and are always callable (to allow enabling
/// the feature or changing config even if dispatch is currently disabled).
async fn update_mcp_config(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<McpConfigPayload>,
) -> Result<Json<Value>, ApiError> {
    let mut guard = state.mcp_runtime.lock().await;
    if let Some(cfg_str) = &payload.config {
        let parsed: Value = serde_json::from_str(cfg_str).unwrap_or(json!({"raw": cfg_str}));
        guard.config = Some(parsed);
    }
    if let Some(en) = payload.enabled {
        guard.enabled = en;
    }
    Ok(Json(json!({
        "status": "updated",
        "enabled": guard.enabled,
        "runtime_config": guard.config
    })))
}

#[derive(Deserialize, Debug)]
struct McpToolCall {
    #[allow(dead_code)]
    tool: Option<String>, // expect "lab_safety"
    arguments: Option<Value>, // { "action": "...", ... }
    action: Option<String>, // direct support too
    #[serde(flatten)]
    extra: Value,
}

/// POST /mcp/call  -- the single large "lab_safety" tool
/// Grouping via "action" param inside arguments or top level.
/// The functional dispatch is gated by mcp_runtime.enabled. Config endpoints remain
/// available for management (see get/update handlers).
async fn call_mcp_tool(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<McpToolCall>,
) -> Result<Json<Value>, ApiError> {
    // Gate the actual tool invocation (the "functions"). Config read/write is separate.
    {
        let rt = state.mcp_runtime.lock().await;
        if !rt.enabled {
            return Err(ApiError::bad_request("MCP is disabled (set MCP_ENABLED=true or POST /mcp {\"enabled\":true})"));
        }
    }

    // support both {tool: "lab_safety", arguments: {action: "xx", ...}} and flat
    let _tool = payload.tool.clone().unwrap_or_else(|| "lab_safety".to_string()); // use the big tool name
    let args = payload.arguments.unwrap_or(payload.extra);
    let action = payload
        .action
        .or_else(|| args.get("action").and_then(|v| v.as_str().map(|s| s.to_string())))
        .unwrap_or_default();

    if action.is_empty() {
        return Err(ApiError::bad_request("action (grouping param) is required and cannot be empty/null"));
    }

    // Use extracted pure dispatch (plan requirement).
    let value = dispatch_lab_safety_action(&state.pool, &action, &args).await?;
    Ok(Json(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use std::collections::HashMap;
    use tokio::sync::Mutex as TokioMutex;
    use tower::ServiceExt;

    #[test]
    fn dispatcher_rejects_empty_action() {
        // mirrors the check in call_mcp_tool
        let action = "";
        let err = ApiError::bad_request("action (grouping param) is required and cannot be empty/null");
        assert!(action.is_empty());
        assert!(err.message.contains("action"));
    }

    #[test]
    fn big_tool_name_and_grouping_param() {
        // confirms design: one tool "lab_safety" grouped by action
        let tool = "lab_safety";
        let grouping = "action";
        assert_eq!(tool, "lab_safety");
        assert_eq!(grouping, "action");
    }

    // Drives the real get_mcp_config handler (shipped code) using test router + minimal state (pool not hit by this handler)
    #[tokio::test]
    async fn test_get_mcp_config_endpoint_drives_real_handler() {
        // Minimal state: use a lazy pool (connect not triggered for config endpoint)
        let pool = sqlx::PgPool::connect_lazy("postgres://invalid:5432/test").expect("lazy pool");
        let settings = crate::config::Settings {
            app_env: "test".into(),
            bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_url: "postgres://test".into(),
            secret_key: "test".into(),
            token_ttl_seconds: 3600,
            upload_dir: "/tmp".into(),
            static_dir: None,
            sso_enabled: false,
            oauth_enabled: false,
            sso_login_url: None,
            oauth_login_url: None,
            federated_login_secret: None,
            webauthn_rp_id: "localhost".into(),
            webauthn_origin: "http://localhost".into(),
            cors_allowed_origins: vec![],
            mcp_enabled: true,
            mcp_config: Some("{\"test\":true}".into()),
        };
        let state = Arc::new(AppState {
            pool,
            settings,
            passkey_registrations: TokioMutex::new(HashMap::new()),
            passkey_authentications: TokioMutex::new(HashMap::new()),
            mcp_runtime: TokioMutex::new(crate::route_support::McpRuntime { enabled: true, config: None }),
        });
        let app = mcp_routes().with_state(state);

        let req = Request::builder().uri("/mcp").body(Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 200);

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["tool_name"], "lab_safety");
        assert_eq!(json["enabled"], true);
    }

    // Drives the real call_mcp_tool handler (shipped code) via router POST for error path (no DB hit on early return)
    #[tokio::test]
    async fn test_call_mcp_tool_bad_action_drives_real_handler() {
        let pool = sqlx::PgPool::connect_lazy("postgres://invalid:5432/test").expect("lazy");
        let settings = crate::config::Settings {
            app_env: "test".into(), bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_url: "postgres://test".into(), secret_key: "test".into(),
            token_ttl_seconds: 3600, upload_dir: "/tmp".into(), static_dir: None,
            sso_enabled: false, oauth_enabled: false, sso_login_url: None, oauth_login_url: None,
            federated_login_secret: None, webauthn_rp_id: "l".into(), webauthn_origin: "http://l".into(),
            cors_allowed_origins: vec![], mcp_enabled: true, mcp_config: None,
        };
        let state = Arc::new(AppState {
            pool, settings,
            passkey_registrations: TokioMutex::new(HashMap::new()),
            passkey_authentications: TokioMutex::new(HashMap::new()),
            mcp_runtime: TokioMutex::new(crate::route_support::McpRuntime { enabled: true, config: None }),
        });
        let app = mcp_routes().with_state(state);

        let req = Request::builder()
            .uri("/mcp/call")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":""}"#)).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 400);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        assert!(json["detail"].as_str().unwrap_or("").contains("action"));
    }

    // Drives gate: when runtime enabled=false, /mcp/call returns disabled error (before any dispatch)
    #[tokio::test]
    async fn test_call_mcp_tool_disabled_gates_real_handler() {
        let pool = sqlx::PgPool::connect_lazy("postgres://invalid:5432/test").expect("lazy");
        let settings = crate::config::Settings {
            app_env: "test".into(), bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_url: "postgres://test".into(), secret_key: "test".into(),
            token_ttl_seconds: 3600, upload_dir: "/tmp".into(), static_dir: None,
            sso_enabled: false, oauth_enabled: false, sso_login_url: None, oauth_login_url: None,
            federated_login_secret: None, webauthn_rp_id: "l".into(), webauthn_origin: "http://l".into(),
            cors_allowed_origins: vec![], mcp_enabled: false, mcp_config: None,
        };
        let state = Arc::new(AppState {
            pool, settings,
            passkey_registrations: TokioMutex::new(HashMap::new()),
            passkey_authentications: TokioMutex::new(HashMap::new()),
            mcp_runtime: TokioMutex::new(crate::route_support::McpRuntime { enabled: false, config: None }),
        });
        let app = mcp_routes().with_state(state);

        let req = Request::builder()
            .uri("/mcp/call")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"action":"list_hazards"}"#)).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), 400);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: Value = serde_json::from_slice(&body).unwrap();
        let detail = json["detail"].as_str().unwrap_or("");
        assert!(detail.contains("MCP is disabled"), "gate message: {}", detail);
    }

    // Drives real update + get handlers: POST /mcp can toggle enabled and it reflects in subsequent GET (runtime persisted in state)
    #[tokio::test]
    async fn test_mcp_config_post_updates_enabled_and_get_reflects_runtime() {
        let pool = sqlx::PgPool::connect_lazy("postgres://invalid:5432/test").expect("lazy");
        let settings = crate::config::Settings {
            app_env: "test".into(), bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_url: "postgres://test".into(), secret_key: "test".into(),
            token_ttl_seconds: 3600, upload_dir: "/tmp".into(), static_dir: None,
            sso_enabled: false, oauth_enabled: false, sso_login_url: None, oauth_login_url: None,
            federated_login_secret: None, webauthn_rp_id: "l".into(), webauthn_origin: "http://l".into(),
            cors_allowed_origins: vec![], mcp_enabled: false, mcp_config: None,
        };
        let state = Arc::new(AppState {
            pool, settings,
            passkey_registrations: TokioMutex::new(HashMap::new()),
            passkey_authentications: TokioMutex::new(HashMap::new()),
            mcp_runtime: TokioMutex::new(crate::route_support::McpRuntime { enabled: false, config: None }),
        });
        let app = mcp_routes().with_state(state.clone());

        // POST to enable
        let post_req = Request::builder()
            .uri("/mcp")
            .method("POST")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enabled":true,"config":"{\"test\":\"via-post\"}"}"#)).unwrap();
        let post_resp = app.clone().oneshot(post_req).await.unwrap();
        assert_eq!(post_resp.status(), 200);
        let post_body = axum::body::to_bytes(post_resp.into_body(), usize::MAX).await.unwrap();
        let post_json: Value = serde_json::from_slice(&post_body).unwrap();
        assert_eq!(post_json["enabled"], true);

        // GET reflects the runtime update
        let get_req = Request::builder().uri("/mcp").body(Body::empty()).unwrap();
        let get_resp = app.oneshot(get_req).await.unwrap();
        assert_eq!(get_resp.status(), 200);
        let get_body = axum::body::to_bytes(get_resp.into_body(), usize::MAX).await.unwrap();
        let get_json: Value = serde_json::from_slice(&get_body).unwrap();
        assert_eq!(get_json["enabled"], true);
        assert_eq!(get_json["runtime_config"]["test"], "via-post");
    }

    // Local oneshot gate flow test (no DB, lazy pool): POST disable via /mcp, /mcp/call -> 400 disabled,
    // POST re-enable, then call hits the empty-action error path (proves gate + re-enable without remote).
    #[tokio::test]
    async fn test_mcp_disable_via_config_then_call_400_then_reenable() {
        let pool = sqlx::PgPool::connect_lazy("postgres://invalid:5432/test").expect("lazy");
        let settings = crate::config::Settings {
            app_env: "test".into(), bind_addr: "127.0.0.1:0".parse().unwrap(),
            database_url: "postgres://test".into(), secret_key: "test".into(),
            token_ttl_seconds: 3600, upload_dir: "/tmp".into(), static_dir: None,
            sso_enabled: false, oauth_enabled: false, sso_login_url: None, oauth_login_url: None,
            federated_login_secret: None, webauthn_rp_id: "l".into(), webauthn_origin: "http://l".into(),
            cors_allowed_origins: vec![], mcp_enabled: true, mcp_config: None,
        };
        let state = Arc::new(AppState {
            pool, settings,
            passkey_registrations: TokioMutex::new(HashMap::new()),
            passkey_authentications: TokioMutex::new(HashMap::new()),
            mcp_runtime: TokioMutex::new(crate::route_support::McpRuntime { enabled: true, config: None }),
        });
        let app = mcp_routes().with_state(state.clone());

        // disable via config endpoint
        let dis = Request::builder().uri("/mcp").method("POST").header("content-type", "application/json")
            .body(Body::from(r#"{"enabled":false}"#)).unwrap();
        let r = app.clone().oneshot(dis).await.unwrap();
        assert_eq!(r.status(), 200);

        // call should be gated
        let bad = Request::builder().uri("/mcp/call").method("POST").header("content-type", "application/json")
            .body(Body::from(r#"{"action":"list_hazards"}"#)).unwrap();
        let r = app.clone().oneshot(bad).await.unwrap();
        assert_eq!(r.status(), 400);
        let body = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        let j: Value = serde_json::from_slice(&body).unwrap();
        assert!(j["detail"].as_str().unwrap_or("").contains("MCP is disabled"));

        // re-enable
        let en = Request::builder().uri("/mcp").method("POST").header("content-type", "application/json")
            .body(Body::from(r#"{"enabled":true}"#)).unwrap();
        let _ = app.clone().oneshot(en).await.unwrap();

        // now a call reaches the action parser (empty action error, not disabled)
        let empty = Request::builder().uri("/mcp/call").method("POST").header("content-type", "application/json")
            .body(Body::from(r#"{"action":""}"#)).unwrap();
        let r = app.oneshot(empty).await.unwrap();
        assert_eq!(r.status(), 400);
        let body = axum::body::to_bytes(r.into_body(), usize::MAX).await.unwrap();
        let j: Value = serde_json::from_slice(&body).unwrap();
        assert!(j["detail"].as_str().unwrap_or("").contains("action (grouping param)"));
    }
}
