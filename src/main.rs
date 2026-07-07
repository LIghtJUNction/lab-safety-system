use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

use axum::http::{HeaderValue, Method, header};
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
        app = app.fallback_service(
            ServeDir::new(&static_dir)
                .not_found_service(ServeFile::new(static_dir.join("index.html"))),
        );
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
