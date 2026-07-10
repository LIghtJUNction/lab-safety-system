use std::sync::Arc;

use axum::{Json, extract::State, http::HeaderMap};
use serde::{Deserialize, Serialize};

use crate::{
    auth_settings::{self, AuthRuntimeSettings},
    models::{CarouselSlide, LoginCarouselSettings, SiteSetting},
    route_permissions::is_system_admin,
    route_support::{ApiError, AppState},
    routes::require_user,
};

pub(crate) async fn get_auth_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        return Err(ApiError::forbidden(
            "Only system administrator can view authentication settings",
        ));
    }

    let runtime = state.auth_runtime.read().await;
    Ok(Json(auth_settings_value(&runtime)?))
}

fn auth_settings_value(runtime: &AuthRuntimeSettings) -> Result<serde_json::Value, ApiError> {
    Ok(serde_json::to_value(AuthSettingsView {
        sso_enabled: runtime.sso_enabled,
        sso_login_url: runtime.sso_login_url.as_deref(),
        oauth_enabled: runtime.oauth_enabled,
        oauth_login_url: runtime.oauth_login_url.as_deref(),
        federated_login_secret_configured: runtime.valid_federated_login_secret().is_some(),
    })?)
}

pub(crate) async fn get_deployment_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<DeploymentSettingsView<'static>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        return Err(ApiError::forbidden(
            "Only system administrator can view deployment settings",
        ));
    }

    Ok(Json(DeploymentSettingsView {
        app_env: state.settings.app_env.clone(),
        token_ttl_seconds: state.settings.token_ttl_seconds,
        webauthn_rp_id: state.settings.webauthn_rp_id.clone(),
        webauthn_origin: state.settings.webauthn_origin.clone(),
        cors_allowed_origins: state.settings.cors_allowed_origins.clone(),
        mcp_enabled: state.settings.mcp_enabled,
        callback_paths: CallbackPaths {
            sso: "/api/v1/auth/sso/callback",
            oauth: "/api/v1/auth/oauth/callback",
        },
    }))
}

#[derive(Serialize)]
pub(crate) struct DeploymentSettingsView<'a> {
    app_env: String,
    token_ttl_seconds: i64,
    webauthn_rp_id: String,
    webauthn_origin: String,
    cors_allowed_origins: Vec<String>,
    mcp_enabled: bool,
    callback_paths: CallbackPaths<'a>,
}

#[derive(Serialize)]
struct CallbackPaths<'a> {
    sso: &'a str,
    oauth: &'a str,
}

#[derive(Serialize)]
struct AuthSettingsView<'a> {
    sso_enabled: bool,
    sso_login_url: Option<&'a str>,
    oauth_enabled: bool,
    oauth_login_url: Option<&'a str>,
    federated_login_secret_configured: bool,
}

#[derive(Deserialize)]
pub(crate) struct AuthSettingsPatch {
    sso_enabled: bool,
    sso_login_url: Option<String>,
    oauth_enabled: bool,
    oauth_login_url: Option<String>,
    federated_login_secret: Option<String>,
    #[serde(default)]
    clear_federated_login_secret: bool,
}

pub(crate) async fn update_auth_settings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<AuthSettingsPatch>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        return Err(ApiError::forbidden(
            "Only system administrator can update authentication settings",
        ));
    }

    let mut runtime = state.auth_runtime.write().await;
    let submitted_secret = payload
        .federated_login_secret
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if submitted_secret
        .is_some_and(|secret| secret.len() < auth_settings::MIN_FEDERATED_SECRET_LENGTH)
    {
        return Err(ApiError::bad_request(
            "Federated login secret must be at least 32 characters",
        ));
    }
    let new_secret = auth_settings::normalize_federated_secret(submitted_secret);
    let secret_configured = if payload.clear_federated_login_secret {
        false
    } else {
        new_secret.is_some() || runtime.valid_federated_login_secret().is_some()
    };
    if payload.sso_enabled {
        validate_login_url(payload.sso_login_url.as_deref(), "SSO")?;
    }
    if payload.oauth_enabled {
        validate_login_url(payload.oauth_login_url.as_deref(), "OAuth")?;
    }
    if (payload.sso_enabled || payload.oauth_enabled) && !secret_configured {
        return Err(ApiError::bad_request(
            "A federated login secret is required when SSO or OAuth is enabled",
        ));
    }
    if payload.clear_federated_login_secret && (payload.sso_enabled || payload.oauth_enabled) {
        return Err(ApiError::bad_request(
            "Federated login secret cannot be cleared while SSO or OAuth is enabled",
        ));
    }
    let candidate = AuthRuntimeSettings {
        sso_enabled: payload.sso_enabled,
        sso_login_url: normalized_url(payload.sso_login_url),
        oauth_enabled: payload.oauth_enabled,
        oauth_login_url: normalized_url(payload.oauth_login_url),
        federated_login_secret: if payload.clear_federated_login_secret {
            None
        } else {
            new_secret.or_else(|| {
                runtime
                    .valid_federated_login_secret()
                    .map(ToString::to_string)
            })
        },
    };
    auth_settings::save(&state.pool, &candidate, &state.settings.secret_key)
        .await
        .map_err(|error| {
            tracing::error!(error = ?error, "authentication settings persistence failed");
            ApiError {
                status: axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                message: "Authentication settings could not be saved".to_string(),
            }
        })?;
    *runtime = candidate;
    Ok(Json(auth_settings_value(&runtime)?))
}

fn normalized_url(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn validate_login_url(value: Option<&str>, provider: &str) -> Result<(), ApiError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Err(ApiError::bad_request(format!(
            "{provider} login URL is required when enabled"
        )));
    };
    let uri = value
        .parse::<axum::http::Uri>()
        .map_err(|_| ApiError::bad_request(format!("{provider} login URL must be valid")))?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) || uri.authority().is_none() {
        return Err(ApiError::bad_request(format!(
            "{provider} login URL must be an absolute HTTP or HTTPS URL"
        )));
    }
    Ok(())
}

pub(crate) async fn get_login_carousel(
    State(state): State<Arc<AppState>>,
) -> Result<Json<LoginCarouselSettings>, ApiError> {
    let row: Option<SiteSetting> = sqlx::query_as(
        "select key, value, updated_at from site_settings where key = 'login_carousel' limit 1",
    )
    .fetch_optional(&state.pool)
    .await?;

    if let Some(row) = row {
        let parsed = serde_json::from_value::<LoginCarouselSettings>(row.value)?;
        if !parsed.zh.is_empty() || !parsed.en.is_empty() {
            return Ok(Json(parsed));
        }
    }
    Ok(Json(default_login_carousel()))
}

pub(crate) async fn update_login_carousel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<LoginCarouselSettings>,
) -> Result<Json<LoginCarouselSettings>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        return Err(ApiError::forbidden(
            "Only system administrator can update login carousel",
        ));
    }
    if payload.zh.is_empty() && payload.en.is_empty() {
        return Err(ApiError::bad_request(
            "At least one language carousel must have slides",
        ));
    }
    let value = serde_json::to_value(&payload)?;
    sqlx::query(
        r#"
        insert into site_settings (key, value, updated_at)
        values ('login_carousel', $1, now())
        on conflict (key) do update set value = excluded.value, updated_at = now()
        "#,
    )
    .bind(value)
    .execute(&state.pool)
    .await?;
    Ok(Json(payload))
}

pub(crate) async fn reset_login_carousel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        return Err(ApiError::forbidden(
            "Only system administrator can reset login carousel",
        ));
    }

    sqlx::query("delete from site_settings where key = 'login_carousel'")
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "reset": true })))
}

fn default_login_carousel() -> LoginCarouselSettings {
    let zh = vec![
        CarouselSlide {
            stat: "隐患闭环".to_string(),
            title: "实验室安全管理平台".to_string(),
            body: "统一处理隐患上报、责任认领、整改照片、培训考核、设备预约和报修工单。"
                .to_string(),
        },
        CarouselSlide {
            stat: "分角色视图".to_string(),
            title: "管理端与普通用户分离".to_string(),
            body: "管理员聚合统计、用户和台账；普通用户只处理自己的上报、认领与整改任务。"
                .to_string(),
        },
        CarouselSlide {
            stat: "安全登录".to_string(),
            title: "支持多种身份入口".to_string(),
            body: "账号密码、Passkey、SSO 和 OAuth 可按部署环境组合使用，超级管理员仍由 CLI 控制。"
                .to_string(),
        },
    ];
    let en = vec![
        CarouselSlide {
            stat: "Hazard closure".to_string(),
            title: "Closed-loop lab safety platform".to_string(),
            body: "Track hazards, ownership, remediation photos, training, bookings, and repair tickets in one workflow.".to_string(),
        },
        CarouselSlide {
            stat: "Role-based UI".to_string(),
            title: "Separate admin and user views".to_string(),
            body: "Administrators manage analytics and registries; normal users focus on their own reports and remediation tasks.".to_string(),
        },
        CarouselSlide {
            stat: "Secure sign-in".to_string(),
            title: "Multiple identity options".to_string(),
            body: "Password, Passkey, SSO, and OAuth can be combined per deployment while super admins stay CLI-governed.".to_string(),
        },
    ];
    LoginCarouselSettings { zh, en }
}
