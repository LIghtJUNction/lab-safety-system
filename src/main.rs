use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::{
    Router,
    extract::Request,
    http::{HeaderValue, Method, header},
    middleware::{self, Next},
    response::Response,
};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::{
    cors::{AllowOrigin, Any, CorsLayer},
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

mod backup;
mod cli;
mod config;
mod db;
mod models;
mod route_analytics;
mod route_auth;
mod route_auth_support;
mod route_documents;
mod route_hazards;
mod route_invitations;
mod route_operations;
mod route_permissions;
mod route_settings;
mod route_support;
mod route_uploads;
mod route_users_labs;
mod routes;
mod security;

use config::Settings;
use route_support::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().any(|arg| arg == "--version") {
        println!("lab-safety-system {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    if cli::try_run(std::env::args().collect()).await? {
        return Ok(());
    }

    if std::env::args().any(|arg| arg == "--healthcheck") {
        return healthcheck();
    }

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let settings = Settings::from_env()?;
    tokio::fs::create_dir_all(&settings.upload_dir).await?;
    let pool = db::connect(&settings.database_url).await?;
    db::migrate(&pool).await?;

    let uploads = settings.upload_dir.clone();
    let state = Arc::new(AppState {
        pool,
        settings: settings.clone(),
        passkey_registrations: Mutex::new(HashMap::new()),
        passkey_authentications: Mutex::new(HashMap::new()),
        mcp_runtime: Mutex::new(crate::route_support::McpRuntime {
            enabled: settings.mcp_enabled,
            config: None,
        }),
    });
    let mut app = routes::router(state)
        .nest_service("/uploads", ServeDir::new(uploads))
        .layer(cors_layer(&settings)?)
        .layer(TraceLayer::new_for_http());

    if let Some(static_dir) = settings.static_dir.clone() {
        app = app.fallback_service(static_files_service(static_dir));
    }

    let listener = TcpListener::bind(settings.bind_addr).await?;
    tracing::info!(
        "lab-safety-system backend listening on {}",
        settings.bind_addr
    );
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn static_files_service(static_dir: PathBuf) -> Router {
    Router::new()
        .fallback_service(
            ServeDir::new(&static_dir)
                .not_found_service(ServeFile::new(static_dir.join("index.html"))),
        )
        .layer(middleware::from_fn(set_static_cache_control))
}

async fn set_static_cache_control(request: Request, next: Next) -> Response {
    let is_asset_path = request.uri().path().starts_with("/assets/");
    let mut response = next.run(request).await;
    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/html"));
    let cache_control = if is_asset_path && response.status().is_success() && !is_html {
        HeaderValue::from_static("public, max-age=31536000, immutable")
    } else {
        HeaderValue::from_static("no-cache")
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, cache_control);
    response
}

fn cors_layer(settings: &Settings) -> anyhow::Result<CorsLayer> {
    if settings.app_env != "production" {
        return Ok(CorsLayer::permissive());
    }

    let mut origins = Vec::with_capacity(settings.cors_allowed_origins.len() + 1);
    origins.push(settings.webauthn_origin.parse::<HeaderValue>()?);
    for origin in &settings.cors_allowed_origins {
        origins.push(origin.parse::<HeaderValue>()?);
    }

    Ok(CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE])
        .expose_headers(Any))
}

fn healthcheck() -> anyhow::Result<()> {
    let port = std::env::var("APP_PORT").unwrap_or_else(|_| "8080".to_string());
    let mut stream = TcpStream::connect(format!("127.0.0.1:{port}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    stream
        .write_all(b"GET /api/v1/ready HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")?;
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    if response.starts_with("HTTP/1.1 200") {
        Ok(())
    } else {
        anyhow::bail!("healthcheck failed")
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, header},
        routing::get,
    };
    use tower::ServiceExt;

    use super::*;

    fn static_test_app() -> (tempfile::TempDir, Router) {
        let static_dir = tempfile::tempdir().expect("create static temp dir");
        std::fs::write(
            static_dir.path().join("index.html"),
            "<main>spa-index</main>",
        )
        .expect("write index");
        std::fs::create_dir(static_dir.path().join("assets")).expect("create assets dir");
        std::fs::write(
            static_dir.path().join("assets/app-hash.js"),
            "window.appHash = true;",
        )
        .expect("write asset");
        let app = static_files_service(static_dir.path().into());
        (static_dir, app)
    }

    async fn static_response(path: &str) -> (HeaderValue, String) {
        let (_static_dir, app) = static_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri(path)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("static response");
        let cache_control = response
            .headers()
            .get(header::CACHE_CONTROL)
            .expect("cache-control header")
            .clone();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("read body");
        (
            cache_control,
            String::from_utf8(body.to_vec()).expect("utf-8 body"),
        )
    }

    fn production_shape_test_app() -> (tempfile::TempDir, tempfile::TempDir, Router) {
        let (static_dir, static_app) = static_test_app();
        let upload_dir = tempfile::tempdir().expect("create upload temp dir");
        std::fs::write(upload_dir.path().join("probe.txt"), "upload-probe")
            .expect("write upload probe");
        let app = Router::new()
            .route("/api/probe", get(|| async { "api-probe" }))
            .nest_service("/uploads", ServeDir::new(upload_dir.path()))
            .fallback_service(static_app);
        (static_dir, upload_dir, app)
    }

    #[tokio::test]
    async fn root_html_should_require_cache_revalidation() {
        let (cache_control, body) = static_response("/").await;

        assert_eq!(cache_control, HeaderValue::from_static("no-cache"));
        assert_eq!(body, "<main>spa-index</main>");
    }

    #[tokio::test]
    async fn spa_fallback_should_return_index_with_cache_revalidation() {
        let (cache_control, body) = static_response("/labs/1/overview").await;

        assert_eq!(cache_control, HeaderValue::from_static("no-cache"));
        assert_eq!(body, "<main>spa-index</main>");
    }

    #[tokio::test]
    async fn hashed_asset_should_use_immutable_cache_and_return_file() {
        let (cache_control, body) = static_response("/assets/app-hash.js").await;

        assert_eq!(
            cache_control,
            HeaderValue::from_static("public, max-age=31536000, immutable")
        );
        assert_eq!(body, "window.appHash = true;");
    }

    #[tokio::test]
    async fn missing_asset_fallback_should_require_cache_revalidation() {
        let (cache_control, body) = static_response("/assets/missing.js").await;

        assert_eq!(cache_control, HeaderValue::from_static("no-cache"));
        assert_eq!(body, "<main>spa-index</main>");
    }

    #[tokio::test]
    async fn matched_api_route_should_not_receive_static_cache_control() {
        let (_static_dir, _upload_dir, app) = production_shape_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/probe")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("api response");

        assert_eq!(response.headers().get(header::CACHE_CONTROL), None);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read body"),
            "api-probe"
        );
    }

    #[tokio::test]
    async fn matched_upload_route_should_not_receive_static_cache_control() {
        let (_static_dir, _upload_dir, app) = production_shape_test_app();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/uploads/probe.txt")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("upload response");

        assert_eq!(response.headers().get(header::CACHE_CONTROL), None);
        assert_eq!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("read body"),
            "upload-probe"
        );
    }
}
