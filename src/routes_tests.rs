use std::collections::HashMap;

use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use sqlx::{Executor, PgPool, postgres::PgPoolOptions};
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

use crate::{config::Settings, db, security::hash_password};

struct TestApp {
    app: Router,
    pool: PgPool,
    schema: String,
    admin_token: String,
    admin_id: i64,
    researcher_token: String,
    researcher_id: i64,
}

async fn test_app() -> anyhow::Result<Option<TestApp>> {
    let Some(database_url) = std::env::var("TEST_DATABASE_URL")
        .ok()
        .or_else(|| std::env::var("DATABASE_URL").ok())
    else {
        eprintln!("skipping postgres integration test: TEST_DATABASE_URL is not set");
        return Ok(None);
    };

    let schema = format!("test_{}", Uuid::new_v4().simple());
    let admin_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&database_url)
        .await?;
    admin_pool
        .execute(format!(r#"create schema "{schema}""#).as_str())
        .await?;

    let search_path = schema.clone();
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(move |connection, _| {
            let search_path = search_path.clone();
            Box::pin(async move {
                connection
                    .execute(format!(r#"set search_path to "{search_path}""#).as_str())
                    .await?;
                Ok(())
            })
        })
        .connect(&database_url)
        .await?;

    db::migrate(&pool).await?;
    let upload_dir = tempfile::tempdir()?.keep();
    let settings = Settings {
        app_env: "test".to_string(),
        bind_addr: "127.0.0.1:0".parse()?,
        database_url,
        secret_key: format!("test-secret-{schema}"),
        token_ttl_seconds: 3600,
        upload_dir,
        static_dir: None,
        sso_enabled: false,
        oauth_enabled: false,
        sso_login_url: None,
        oauth_login_url: None,
        federated_login_secret: None,
        webauthn_rp_id: "localhost".to_string(),
        webauthn_origin: "http://localhost:5174".to_string(),
        cors_allowed_origins: vec![],
        mcp_enabled: false,
        mcp_config: None,
    };

    let admin_password_hash = hash_password("AdminStrong123!");
    let researcher_password_hash = hash_password("ResearcherStrong123!");
    let admin_id: i64 = sqlx::query_scalar(
        r#"
            insert into users (username, display_name, email, role, auth_provider, password_hash)
            values ('admin', 'Admin', 'admin@example.com', 'system_admin', 'password', $1)
            returning id
            "#,
    )
    .bind(admin_password_hash)
    .fetch_one(&pool)
    .await?;
    let researcher_id: i64 = sqlx::query_scalar(
            r#"
            insert into users (username, display_name, email, role, auth_provider, password_hash)
            values ('researcher', 'Researcher', 'researcher@example.com', 'lab_member', 'password', $1)
            returning id
            "#,
        )
        .bind(researcher_password_hash)
        .fetch_one(&pool)
        .await?;

    let state = Arc::new(AppState {
        pool: pool.clone(),
        settings,
        passkey_registrations: Mutex::new(HashMap::new()),
        passkey_authentications: Mutex::new(HashMap::new()),
        mcp_config: Mutex::new(None),
    });
    let app = router(state.clone());
    let admin_token = crate::security::create_access_token(
        "admin",
        &state.settings.secret_key,
        state.settings.token_ttl_seconds,
    )?;
    let researcher_token = crate::security::create_access_token(
        "researcher",
        &state.settings.secret_key,
        state.settings.token_ttl_seconds,
    )?;
    assert!(admin_id > 0);

    Ok(Some(TestApp {
        app,
        pool,
        schema,
        admin_token,
        admin_id,
        researcher_token,
        researcher_id,
    }))
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    body: Body,
    content_type: Option<&str>,
) -> anyhow::Result<(StatusCode, serde_json::Value)> {
    let mut builder = Request::builder().method(method).uri(path);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    if let Some(content_type) = content_type {
        builder = builder.header(header::CONTENT_TYPE, content_type);
    }
    let response = app.clone().oneshot(builder.body(body)?).await?;
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX).await?;
    let value = if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes)?
    };
    Ok((status, value))
}

async fn json_request(
    app: &Router,
    method: Method,
    path: &str,
    token: Option<&str>,
    payload: serde_json::Value,
) -> anyhow::Result<(StatusCode, serde_json::Value)> {
    request(
        app,
        method,
        path,
        token,
        Body::from(payload.to_string()),
        Some("application/json"),
    )
    .await
}

async fn upload(
    app: &Router,
    path: &str,
    token: &str,
    filename: &str,
    content: &str,
) -> anyhow::Result<(StatusCode, serde_json::Value)> {
    let boundary = "x-test-boundary";
    let content_type = test_upload_content_type(filename);
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n{content}\r\n--{boundary}--\r\n"
    );
    request(
        app,
        Method::POST,
        path,
        Some(token),
        Body::from(body),
        Some(&format!("multipart/form-data; boundary={boundary}")),
    )
    .await
}

fn test_upload_content_type(filename: &str) -> &'static str {
    match filename.rsplit('.').next().unwrap_or_default() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "csv" => "text/csv",
        "md" => "text/markdown",
        _ => "text/plain",
    }
}

#[path = "routes_tests/auth_provider_flow.rs"]
mod auth_provider_flow;
#[path = "routes_tests/invitation_flow.rs"]
mod invitation_flow;
#[path = "routes_tests/required_fields_flow.rs"]
mod required_fields_flow;
#[path = "routes_tests/safety_flow.rs"]
mod safety_flow;
#[path = "routes_tests/safety_flow_assertions.rs"]
mod safety_flow_assertions;
#[path = "routes_tests/upload_flow.rs"]
mod upload_flow;
#[path = "routes_tests/upload_url_flow.rs"]
mod upload_url_flow;
