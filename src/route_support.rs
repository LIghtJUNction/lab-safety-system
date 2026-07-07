use std::collections::HashMap;

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::Mutex;
use webauthn_rs::prelude::{
    Passkey, PasskeyAuthentication, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential,
};

use crate::config::Settings;
use serde_json::Value;

pub struct AppState {
    pub pool: PgPool,
    pub settings: Settings,
    pub passkey_registrations: Mutex<PasskeyRegistrationCache>,
    pub passkey_authentications: Mutex<PasskeyAuthenticationCache>,
    pub mcp_config: Mutex<Option<Value>>,
}

pub(crate) type PasskeyRegistrationCache = HashMap<String, (i64, PasskeyRegistration)>;
pub(crate) type PasskeyAuthenticationCache =
    HashMap<String, (String, PasskeyAuthentication, Vec<StoredPasskey>)>;

pub(crate) const ROLE_SYSTEM_ADMIN: &str = "system_admin";
pub(crate) const ROLE_LAB_ADMIN: &str = "lab_admin";
pub(crate) const ROLE_LAB_MEMBER: &str = "lab_member";
pub(crate) const ROLE_VISITOR: &str = "visitor";

#[derive(Debug)]
pub(crate) struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
}

impl ApiError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    pub(crate) fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
}

impl<E> From<E> for ApiError
where
    E: std::error::Error,
{
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "detail": self.message })),
        )
            .into_response()
    }
}

#[derive(Deserialize)]
pub(crate) struct ListQuery {
    pub(crate) q: Option<String>,
    pub(crate) status: Option<String>,
    pub(crate) role: Option<String>,
    pub(crate) lab_id: Option<i64>,
    pub(crate) responsible_user_id: Option<i64>,
    pub(crate) reported_by: Option<i64>,
    pub(crate) limit: Option<i64>,
    pub(crate) offset: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct FederatedLoginQuery {
    pub(crate) username: Option<String>,
    pub(crate) email: Option<String>,
    pub(crate) display_name: Option<String>,
    pub(crate) role: Option<String>,
    pub(crate) department: Option<String>,
    pub(crate) exp: Option<i64>,
    pub(crate) sig: Option<String>,
    pub(crate) redirect: Option<String>,
}

#[derive(Clone)]
pub(crate) struct StoredPasskey {
    pub(crate) id: i64,
    pub(crate) credential: Passkey,
}

#[derive(Deserialize)]
pub(crate) struct PasskeyStartRequest {
    pub(crate) username: String,
}

#[derive(Deserialize)]
pub(crate) struct PasskeyRegisterFinish {
    pub(crate) challenge_id: String,
    pub(crate) name: Option<String>,
    pub(crate) credential: RegisterPublicKeyCredential,
}

#[derive(Deserialize)]
pub(crate) struct PasskeyLoginFinish {
    pub(crate) challenge_id: String,
    pub(crate) credential: PublicKeyCredential,
}

#[derive(Serialize)]
pub(crate) struct PasskeyChallenge<T> {
    pub(crate) challenge_id: String,
    pub(crate) options: T,
}

#[derive(Serialize)]
pub(crate) struct PasskeySummary {
    pub(crate) id: i64,
    pub(crate) name: String,
    pub(crate) created_at: chrono::DateTime<chrono::Utc>,
    pub(crate) last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub(crate) fn validate_federated_role(role: &str) -> Result<(), ApiError> {
    match role {
        ROLE_LAB_MEMBER | ROLE_VISITOR => Ok(()),
        _ => Err(ApiError::bad_request(
            "Federated login role must be lab_member or visitor",
        )),
    }
}

pub(crate) fn federated_signature_message(
    provider: &str,
    username: &str,
    email: &str,
    display_name: &str,
    role: &str,
    department: &str,
    exp: i64,
) -> String {
    format!("{provider}\n{username}\n{email}\n{display_name}\n{role}\n{department}\n{exp}")
}

pub(crate) fn required_federated_param(
    value: Option<String>,
    field: &str,
) -> Result<String, ApiError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request(format!("Missing federated login field: {field}")))
}

pub(crate) fn safe_local_redirect(value: Option<&str>) -> Result<String, ApiError> {
    match value {
        Some(value) if value.starts_with('/') && !value.starts_with("//") => Ok(value.to_string()),
        Some(_) => Err(ApiError::bad_request("Redirect must be a local path")),
        None => Ok("/".to_string()),
    }
}

pub(crate) fn validate_global_role(role: &str) -> Result<(), ApiError> {
    if matches!(role, ROLE_LAB_MEMBER | ROLE_VISITOR) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "API user role must be lab_member or visitor",
        ))
    }
}

pub(crate) fn validate_lab_role(role: &str) -> Result<(), ApiError> {
    if matches!(role, ROLE_LAB_ADMIN | ROLE_LAB_MEMBER | ROLE_VISITOR) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Lab role must be lab_admin, lab_member, or visitor",
        ))
    }
}

pub(crate) fn validate_lab_status(status: &str) -> Result<(), ApiError> {
    if matches!(status, "active" | "inactive" | "maintenance") {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Lab status must be active, inactive, or maintenance",
        ))
    }
}

pub(crate) fn validate_hazard_status(status: &str) -> Result<(), ApiError> {
    if matches!(
        status,
        "open" | "claimed" | "remediation_submitted" | "closed"
    ) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Hazard status must be open, claimed, remediation_submitted, or closed",
        ))
    }
}

pub(crate) fn validate_repair_status(status: &str) -> Result<(), ApiError> {
    if matches!(status, "open" | "in_progress" | "resolved" | "closed") {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Repair status must be open, in_progress, resolved, or closed",
        ))
    }
}

pub(crate) fn validate_repair_create_status(status: &str) -> Result<(), ApiError> {
    if status == "open" {
        Ok(())
    } else {
        Err(ApiError::bad_request("Repair tickets must start as open"))
    }
}

pub(crate) fn validate_optional_upload_url(
    value: Option<&str>,
    prefix: &str,
    field: &str,
) -> Result<(), ApiError> {
    if let Some(value) = value {
        validate_upload_url(value, prefix, field)?;
    }
    Ok(())
}

pub(crate) fn validate_upload_url(value: &str, prefix: &str, field: &str) -> Result<(), ApiError> {
    if value.is_empty()
        || value.trim() != value
        || !value.starts_with(prefix)
        || value.contains("..")
        || value.contains('\\')
    {
        return Err(ApiError::bad_request(format!(
            "{field} must be a URL returned by the upload endpoint"
        )));
    }
    Ok(())
}

pub(crate) fn wildcard(q: Option<String>) -> Option<String> {
    q.filter(|value| !value.trim().is_empty())
        .map(|value| format!("%{}%", value.trim()))
}

pub(crate) fn limit(value: Option<i64>) -> i64 {
    value.unwrap_or(50).clamp(1, 100)
}

pub(crate) fn offset(value: Option<i64>) -> i64 {
    value.unwrap_or(0).max(0)
}
