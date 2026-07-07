use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::HeaderMap,
    routing::get,
};
use sqlx::Row;

use crate::route_support::*;
use crate::{models::*, security::verify_access_token};

mod analytics;
mod auth;
mod documents;
mod hazards;
mod invitations;
mod operations;
mod settings;
mod users_labs;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .merge(health_routes())
        .merge(auth::auth_routes())
        .merge(settings::settings_routes())
        .merge(users_labs::users_labs_routes())
        .merge(invitations::invitations_routes())
        .merge(documents::documents_routes())
        .merge(operations::operations_routes())
        .merge(hazards::hazards_routes())
        .merge(analytics::analytics_routes())
        .with_state(state)
}

fn health_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/ready", get(ready))
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn ready(State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("select 1").execute(&state.pool).await?;
    Ok(Json(serde_json::json!({ "status": "ready" })))
}

pub(crate) async fn require_user(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<AuthUser, ApiError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::unauthorized("Missing bearer token"))?;
    let username = verify_access_token(token, &state.settings.secret_key)
        .map_err(|_| ApiError::unauthorized("Invalid bearer token"))?;
    let row = sqlx::query(
        r#"
        select id, username, display_name, email, role, auth_provider, is_active
        from users
        where username = $1
        "#,
    )
    .bind(&username)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(ApiError::unauthorized("Token user no longer exists"));
    };
    if !row.try_get::<bool, _>("is_active")? {
        return Err(ApiError::unauthorized("User is disabled"));
    }
    Ok(AuthUser {
        id: row.try_get("id")?,
        username: row.try_get("username")?,
        display_name: row.try_get("display_name")?,
        email: row.try_get("email")?,
        role: row.try_get("role")?,
        auth_provider: row.try_get("auth_provider")?,
    })
}

#[cfg(test)]
#[path = "../routes_tests.rs"]
mod routes_tests;
