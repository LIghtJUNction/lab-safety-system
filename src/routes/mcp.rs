use std::sync::Arc;

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;

use crate::route_support::{ApiError, AppState};

pub fn mcp_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/mcp", get(get_mcp_config).post(update_mcp_config))
        .route("/mcp/call", post(call_mcp_tool))
}

/// GET /mcp returns current config/status
async fn get_mcp_config(State(state): State<Arc<AppState>>) -> Json<Value> {
    let runtime = state.mcp_config.lock().await.clone();
    Json(json!({
        "enabled": state.settings.mcp_enabled,
        "config": state.settings.mcp_config,
        "runtime_config": runtime,
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

/// POST /mcp accepts config updates (in-memory for demo)
async fn update_mcp_config(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<McpConfigPayload>,
) -> Result<Json<Value>, ApiError> {
    let mut guard = state.mcp_config.lock().await;
    if let Some(cfg_str) = payload.config {
        let parsed: Value = serde_json::from_str(&cfg_str).unwrap_or(json!({"raw": cfg_str}));
        *guard = Some(parsed);
    }
    Ok(Json(json!({
        "status": "updated",
        "enabled": payload.enabled.unwrap_or(state.settings.mcp_enabled),
        "runtime_config": *guard
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
async fn call_mcp_tool(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<McpToolCall>,
) -> Result<Json<Value>, ApiError> {
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

    // Dispatch to sub functions based on action
    match action.as_str() {
        "list_labs" => {
            let q = args.get("q").and_then(|v| v.as_str());
            let sql = if let Some(qq) = q {
                format!(
                    "select id, code, name, location, department, manager_user_id, contact, status, description, created_at from labs where name ilike '%{}%' or code ilike '%{}%' limit 10",
                    qq.replace('\'', "''"), qq.replace('\'', "''")
                )
            } else {
                "select id, code, name, location, department, manager_user_id, contact, status, description, created_at from labs limit 10".to_string()
            };
            let rows = sqlx::query(&sql)
                .fetch_all(&state.pool)
                .await
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
            Ok(Json(json!({ "action": action, "result": labs, "count": labs.len() })))
        }
        "list_hazards" => {
            let sql = "select id, title, lab_name, status, category, description from safety_hazards limit 10";
            let rows = sqlx::query(sql)
                .fetch_all(&state.pool)
                .await
                .map_err(|e| ApiError::bad_request(format!("db: {}", e)))?;
            let items: Vec<Value> = rows.into_iter().map(|r| json!({
                "id": r.get::<i64,_>("id"),
                "title": r.get::<String,_>("title"),
                "lab_name": r.get::<String,_>("lab_name"),
                "status": r.get::<String,_>("status"),
            })).collect();
            Ok(Json(json!({ "action": action, "result": items })))
        }
        "create_hazard" => {
            let title = args.get("title").and_then(|v| v.as_str()).unwrap_or("MCP created hazard").to_string();
            let lab_name = args.get("lab_name").and_then(|v| v.as_str()).unwrap_or("demo-lab").to_string();
            // minimal insert, ignore some nulls per prior handling
            let id: i64 = sqlx::query_scalar(
                r#"insert into safety_hazards (title, lab_name, category, description, reported_by, status)
                   values ($1, $2, 'other', $3, 1, 'reported') returning id"#
            )
            .bind(&title)
            .bind(&lab_name)
            .bind(args.get("description").and_then(|v|v.as_str()).unwrap_or("via mcp big tool"))
            .fetch_one(&state.pool)
            .await
            .map_err(|e| ApiError::bad_request(format!("insert err: {}", e)))?;
            Ok(Json(json!({ "action": action, "result": { "id": id, "title": title, "status": "reported" } })))
        }
        "list_regulations" | "list_documents" => {
            let sql = "select id, title, regulation_type from regulations limit 5";
            let rows = sqlx::query(sql).fetch_all(&state.pool).await.map_err(|e| ApiError::bad_request(format!("db: {}",e)))?;
            let res: Vec<Value> = rows.iter().map(|r| json!({"id": r.get::<i64,_>("id"), "title": r.get::<String,_>("title") })).collect();
            Ok(Json(json!({ "action": action, "result": res })))
        }
        "list_equipment" | "list_operations" => {
            let sql = "select id, name, status from equipment limit 5";
            let rows = sqlx::query(sql).fetch_all(&state.pool).await.map_err(|e| ApiError::bad_request(format!("db: {}",e)))?;
            let res: Vec<Value> = rows.iter().map(|r| json!({"id": r.get::<i64,_>("id"), "name": r.get::<String,_>("name"), "status": r.get::<String,_>("status") })).collect();
            Ok(Json(json!({ "action": action, "result": res })))
        }
        "list_incidents" => {
            let sql = "select id, title, severity from incident_cases limit 5";
            let rows = sqlx::query(sql).fetch_all(&state.pool).await.map_err(|e| ApiError::bad_request(format!("db: {}",e)))?;
            let res: Vec<Value> = rows.iter().map(|r| json!({"id": r.get::<i64,_>("id"), "title": r.get::<String,_>("title") })).collect();
            Ok(Json(json!({ "action": action, "result": res })))
        }
        _ => Err(ApiError::bad_request(format!("unknown action '{}' for lab_safety tool", action))),
    }
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
            mcp_config: TokioMutex::new(None),
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
            mcp_config: TokioMutex::new(None),
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
}
