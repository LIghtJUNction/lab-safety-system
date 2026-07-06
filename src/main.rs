use std::{
    io::{Read, Write},
    net::TcpStream,
    sync::Arc,
    time::Duration,
};

use tokio::net::TcpListener;
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

mod config;
mod db;
mod models;
mod routes;
mod security;

use config::Settings;
use routes::AppState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
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
    });
    let mut app = routes::router(state)
        .nest_service("/uploads", ServeDir::new(uploads))
        .layer(CorsLayer::permissive())
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
