use std::{collections::HashMap, path::Path, sync::Arc};

use axum::{
    Json, Router,
    extract::{Multipart, Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, patch, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use tokio::{fs, sync::Mutex};
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, PasskeyAuthentication, PasskeyRegistration,
    PublicKeyCredential, RegisterPublicKeyCredential, RequestChallengeResponse, Url as WebauthnUrl,
    Uuid as WebauthnUuid, Webauthn, WebauthnBuilder,
};

use crate::{
    config::Settings,
    models::*,
    security::{
        create_access_token, hash_password, validate_password_strength, verify_access_token,
        verify_message_signature, verify_password,
    },
};

pub struct AppState {
    pub pool: PgPool,
    pub settings: Settings,
    pub passkey_registrations: Mutex<PasskeyRegistrationCache>,
    pub passkey_authentications: Mutex<PasskeyAuthenticationCache>,
}

type PasskeyRegistrationCache = HashMap<String, (i64, PasskeyRegistration)>;
type PasskeyAuthenticationCache =
    HashMap<String, (String, PasskeyAuthentication, Vec<StoredPasskey>)>;

const ROLE_SYSTEM_ADMIN: &str = "system_admin";
const ROLE_LAB_ADMIN: &str = "lab_admin";
const ROLE_LAB_MEMBER: &str = "lab_member";
const ROLE_VISITOR: &str = "visitor";

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
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
pub struct ListQuery {
    q: Option<String>,
    status: Option<String>,
    role: Option<String>,
    lab_id: Option<i64>,
    responsible_user_id: Option<i64>,
    reported_by: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[derive(Deserialize)]
pub struct FederatedLoginQuery {
    username: Option<String>,
    email: Option<String>,
    display_name: Option<String>,
    role: Option<String>,
    department: Option<String>,
    exp: Option<i64>,
    sig: Option<String>,
    redirect: Option<String>,
}

#[derive(Clone)]
pub struct StoredPasskey {
    id: i64,
    credential: Passkey,
}

#[derive(Deserialize)]
pub struct PasskeyStartRequest {
    username: String,
}

#[derive(Deserialize)]
pub struct PasskeyRegisterFinish {
    challenge_id: String,
    name: Option<String>,
    credential: RegisterPublicKeyCredential,
}

#[derive(Deserialize)]
pub struct PasskeyLoginFinish {
    challenge_id: String,
    credential: PublicKeyCredential,
}

#[derive(Serialize)]
pub struct PasskeyChallenge<T> {
    challenge_id: String,
    options: T,
}

#[derive(Serialize)]
pub struct PasskeySummary {
    id: i64,
    name: String,
    created_at: chrono::DateTime<chrono::Utc>,
    last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/ready", get(ready))
        .route("/api/v1/auth/methods", get(auth_methods))
        .route("/api/v1/auth/password-login", post(password_login))
        .route(
            "/api/v1/auth/passkey/login/start",
            post(passkey_login_start),
        )
        .route(
            "/api/v1/auth/passkey/login/finish",
            post(passkey_login_finish),
        )
        .route(
            "/api/v1/auth/passkey/register/start",
            post(passkey_register_start),
        )
        .route(
            "/api/v1/auth/passkey/register/finish",
            post(passkey_register_finish),
        )
        .route("/api/v1/auth/passkeys", get(list_passkeys))
        .route("/api/v1/auth/sso/callback", get(sso_callback))
        .route("/api/v1/auth/oauth/callback", get(oauth_callback))
        .route("/api/v1/auth/me", get(auth_me))
        .route("/api/v1/auth/my-labs", get(my_labs))
        .route("/api/v1/users", get(list_users).post(create_user))
        .route("/api/v1/labs", get(list_labs).post(create_lab))
        .route("/api/v1/labs/{id}", get(get_lab).patch(update_lab))
        .route(
            "/api/v1/labs/{id}/users",
            get(list_lab_users).post(assign_lab_user),
        )
        .route("/api/v1/labs/{id}/users/{user_id}", delete(remove_lab_user))
        .route(
            "/api/v1/regulations",
            get(list_regulations).post(create_regulation),
        )
        .route("/api/v1/regulations/upload", post(upload_regulation_file))
        .route(
            "/api/v1/incidents",
            get(list_incidents).post(create_incident),
        )
        .route("/api/v1/incidents/upload", post(upload_incident_file))
        .route(
            "/api/v1/trainings",
            get(list_trainings).post(create_training),
        )
        .route(
            "/api/v1/exam-results",
            get(list_exam_results).post(create_exam_result),
        )
        .route(
            "/api/v1/equipment",
            get(list_equipment).post(create_equipment),
        )
        .route(
            "/api/v1/equipment-bookings",
            get(list_bookings).post(create_booking),
        )
        .route(
            "/api/v1/repair-tickets",
            get(list_repairs).post(create_repair),
        )
        .route("/api/v1/repair-tickets/{id}", patch(update_repair))
        .route("/api/v1/hazards", get(list_hazards).post(create_hazard))
        .route("/api/v1/hazards/{id}/claim", post(claim_hazard))
        .route("/api/v1/hazards/{id}/remediation", post(remediate_hazard))
        .route("/api/v1/hazards/{id}/status", patch(update_hazard_status))
        .route(
            "/api/v1/hazards/upload/issue-photo",
            post(upload_hazard_issue_photo),
        )
        .route(
            "/api/v1/hazards/upload/remediation-photo",
            post(upload_hazard_remediation_photo),
        )
        .route("/api/v1/analytics/dashboard", get(dashboard_stats))
        .route("/api/v1/analytics/regulations", get(regulation_analytics))
        .route("/api/v1/analytics/incidents", get(incident_analytics))
        .route("/api/v1/analytics/hazards", get(hazard_analytics))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn ready(State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("select 1").execute(&state.pool).await?;
    Ok(Json(serde_json::json!({ "status": "ready" })))
}

async fn auth_methods(State(state): State<Arc<AppState>>) -> Json<AuthMethods> {
    let sso_login_url = federated_login_url(
        state.settings.sso_enabled,
        state.settings.sso_login_url.as_deref(),
        "/api/v1/auth/sso/callback",
    );
    let oauth_login_url = federated_login_url(
        state.settings.oauth_enabled,
        state.settings.oauth_login_url.as_deref(),
        "/api/v1/auth/oauth/callback",
    );
    Json(AuthMethods {
        password: true,
        sso: sso_login_url.is_some(),
        oauth: oauth_login_url.is_some(),
        sso_login_url,
        oauth_login_url,
    })
}

fn federated_login_url(
    enabled: bool,
    login_url: Option<&str>,
    callback_path: &str,
) -> Option<String> {
    if !enabled {
        return None;
    }
    let login_url = login_url?.trim();
    if login_url.is_empty() || login_url == callback_path {
        return None;
    }
    Some(login_url.to_string())
}

async fn password_login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PasswordLogin>,
) -> Result<Json<AuthToken>, ApiError> {
    let row = sqlx::query(
        r#"
        select id, username, display_name, email, role, auth_provider, password_hash, is_active
        from users
        where username = $1
        "#,
    )
    .bind(&payload.username)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid username or password".into(),
        });
    };
    let password_hash: Option<String> = row.try_get("password_hash")?;
    let active: bool = row.try_get("is_active")?;
    if !active || !verify_password(&payload.password, password_hash.as_deref()) {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid username or password".into(),
        });
    }
    let token = create_access_token(
        row.try_get::<String, _>("username")?.as_str(),
        &state.settings.secret_key,
        state.settings.token_ttl_seconds,
    )
    .map_err(|error| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: error.to_string(),
    })?;
    Ok(Json(AuthToken {
        access_token: token,
        token_type: "bearer",
        expires_in: state.settings.token_ttl_seconds,
        user: AuthUser {
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            display_name: row.try_get("display_name")?,
            email: row.try_get("email")?,
            role: row.try_get("role")?,
            auth_provider: row.try_get("auth_provider")?,
        },
    }))
}

async fn passkey_login_start(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PasskeyStartRequest>,
) -> Result<Json<PasskeyChallenge<RequestChallengeResponse>>, ApiError> {
    let username = payload.username.trim();
    if username.is_empty() {
        return Err(ApiError::bad_request("username is required"));
    }
    let passkeys = load_passkeys_for_username(&state, username).await?;
    if passkeys.is_empty() {
        return Err(ApiError::not_found("No passkey is bound to this user"));
    }
    let credentials: Vec<Passkey> = passkeys
        .iter()
        .map(|stored| stored.credential.clone())
        .collect();
    let webauthn = webauthn(&state.settings)?;
    let (options, auth_state) = webauthn
        .start_passkey_authentication(&credentials)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let challenge_id = Uuid::new_v4().to_string();
    state.passkey_authentications.lock().await.insert(
        challenge_id.clone(),
        (username.to_string(), auth_state, passkeys),
    );
    Ok(Json(PasskeyChallenge {
        challenge_id,
        options,
    }))
}

async fn passkey_login_finish(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PasskeyLoginFinish>,
) -> Result<Json<AuthToken>, ApiError> {
    let Some((username, auth_state, stored_passkeys)) = state
        .passkey_authentications
        .lock()
        .await
        .remove(&payload.challenge_id)
    else {
        return Err(ApiError::bad_request("Passkey challenge expired"));
    };
    let webauthn = webauthn(&state.settings)?;
    let auth_result = webauthn
        .finish_passkey_authentication(&payload.credential, &auth_state)
        .map_err(|error| ApiError::unauthorized(error.to_string()))?;
    let mut matched: Option<StoredPasskey> = None;
    for mut stored in stored_passkeys {
        if stored.credential.update_credential(&auth_result).is_some() {
            matched = Some(stored);
            break;
        }
    }
    let Some(stored) = matched else {
        return Err(ApiError::unauthorized(
            "Passkey credential is not registered",
        ));
    };
    sqlx::query("update passkeys set credential_json = $1, last_used_at = now() where id = $2")
        .bind(serde_json::to_string(&stored.credential)?)
        .bind(stored.id)
        .execute(&state.pool)
        .await?;
    let user = load_auth_user_by_username(&state, &username).await?;
    auth_token_for_user(&state, user)
        .map(Json)
        .map_err(|error| ApiError {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        })
}

async fn passkey_register_start(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<PasskeyChallenge<CreationChallengeResponse>>, ApiError> {
    let user = require_user(&state, &headers).await?;
    let exclude_credentials = load_passkeys_for_user(&state, user.id)
        .await?
        .into_iter()
        .map(|stored| stored.credential.cred_id().clone())
        .collect();
    let webauthn = webauthn(&state.settings)?;
    let (options, reg_state) = webauthn
        .start_passkey_registration(
            WebauthnUuid::from_u128(user.id as u128),
            &user.username,
            &user.display_name,
            Some(exclude_credentials),
        )
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let challenge_id = Uuid::new_v4().to_string();
    state
        .passkey_registrations
        .lock()
        .await
        .insert(challenge_id.clone(), (user.id, reg_state));
    Ok(Json(PasskeyChallenge {
        challenge_id,
        options,
    }))
}

async fn passkey_register_finish(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<PasskeyRegisterFinish>,
) -> Result<Json<PasskeySummary>, ApiError> {
    let user = require_user(&state, &headers).await?;
    let Some((user_id, reg_state)) = state
        .passkey_registrations
        .lock()
        .await
        .remove(&payload.challenge_id)
    else {
        return Err(ApiError::bad_request("Passkey challenge expired"));
    };
    if user_id != user.id {
        return Err(ApiError::forbidden(
            "Passkey challenge belongs to another user",
        ));
    }
    let webauthn = webauthn(&state.settings)?;
    let passkey = webauthn
        .finish_passkey_registration(&payload.credential, &reg_state)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let credential_id = serde_json::to_string(passkey.cred_id())?;
    let name = payload
        .name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "Passkey".to_string());
    let row = sqlx::query(
        r#"
        insert into passkeys (user_id, credential_id, name, credential_json)
        values ($1, $2, $3, $4)
        returning id, name, created_at, last_used_at
        "#,
    )
    .bind(user.id)
    .bind(credential_id)
    .bind(name)
    .bind(serde_json::to_string(&passkey)?)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(PasskeySummary {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
    }))
}

async fn list_passkeys(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<PasskeySummary>>, ApiError> {
    let user = require_user(&state, &headers).await?;
    let rows = sqlx::query(
        r#"
        select id, name, created_at, last_used_at
        from passkeys
        where user_id = $1
        order by created_at desc
        "#,
    )
    .bind(user.id)
    .fetch_all(&state.pool)
    .await?;
    let passkeys = rows
        .into_iter()
        .map(|row| {
            Ok(PasskeySummary {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                created_at: row.try_get("created_at")?,
                last_used_at: row.try_get("last_used_at")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(Json(passkeys))
}

async fn sso_callback(
    State(state): State<Arc<AppState>>,
    Query(payload): Query<FederatedLoginQuery>,
) -> Result<Html<String>, ApiError> {
    federated_callback(&state, "sso", state.settings.sso_enabled, payload).await
}

async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    Query(payload): Query<FederatedLoginQuery>,
) -> Result<Html<String>, ApiError> {
    federated_callback(&state, "oauth", state.settings.oauth_enabled, payload).await
}

async fn federated_callback(
    state: &AppState,
    provider: &'static str,
    enabled: bool,
    payload: FederatedLoginQuery,
) -> Result<Html<String>, ApiError> {
    if !enabled {
        return Err(ApiError::forbidden(format!(
            "{provider} login is not enabled"
        )));
    }
    let username = required_federated_param(payload.username, "username")?;
    let email = required_federated_param(payload.email, "email")?;
    let exp = payload
        .exp
        .ok_or_else(|| ApiError::bad_request("Missing federated login field: exp"))?;
    let sig = required_federated_param(payload.sig, "sig")?;

    if exp < chrono::Utc::now().timestamp() {
        return Err(ApiError::unauthorized("Federated login payload expired"));
    }
    let Some(secret) = state.settings.federated_login_secret.as_deref() else {
        return Err(ApiError::forbidden(
            "FEDERATED_LOGIN_SECRET is required for SSO/OAuth callbacks",
        ));
    };
    let display_name = payload
        .display_name
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| username.clone());
    let role = payload
        .role
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| ROLE_LAB_MEMBER.to_string());
    validate_federated_role(&role)?;
    let message = federated_signature_message(
        provider,
        &username,
        &email,
        &display_name,
        &role,
        payload.department.as_deref().unwrap_or(""),
        exp,
    );
    if !verify_message_signature(&message, &sig, secret) {
        return Err(ApiError::unauthorized("Invalid federated login signature"));
    }
    let user = upsert_federated_user(
        state,
        &username,
        &display_name,
        &email,
        &role,
        provider,
        payload.department.as_deref(),
    )
    .await?;
    if !user.is_active {
        return Err(ApiError::unauthorized("User is disabled"));
    }
    let session = auth_token_for_user(state, user.into()).map_err(|error| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: error.to_string(),
    })?;
    let session_json = serde_json::to_string(&session)?;
    let session_payload = URL_SAFE_NO_PAD.encode(session_json.as_bytes());
    let redirect = safe_local_redirect(payload.redirect.as_deref())?;
    let redirect_with_session = format!("{redirect}#session={session_payload}");
    Ok(Html(format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head><meta charset="utf-8"><title>登录成功</title></head>
<body>
<script>
window.location.replace({redirect});
</script>
</body>
</html>"#,
        redirect = serde_json::to_string(&redirect_with_session)?
    )))
}

fn webauthn(settings: &Settings) -> Result<Webauthn, ApiError> {
    let origin = WebauthnUrl::parse(&settings.webauthn_origin)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    WebauthnBuilder::new(&settings.webauthn_rp_id, &origin)
        .map_err(|error| ApiError::bad_request(error.to_string()))?
        .build()
        .map_err(|error| ApiError::bad_request(error.to_string()))
}

async fn load_passkeys_for_username(
    state: &AppState,
    username: &str,
) -> Result<Vec<StoredPasskey>, ApiError> {
    let rows = sqlx::query(
        r#"
        select passkeys.id, passkeys.credential_json
        from passkeys
        join users on users.id = passkeys.user_id
        where users.username = $1 and users.is_active = true
        order by passkeys.created_at desc
        "#,
    )
    .bind(username)
    .fetch_all(&state.pool)
    .await?;
    stored_passkeys_from_rows(rows)
}

async fn load_passkeys_for_user(
    state: &AppState,
    user_id: i64,
) -> Result<Vec<StoredPasskey>, ApiError> {
    let rows = sqlx::query(
        r#"
        select id, credential_json
        from passkeys
        where user_id = $1
        order by created_at desc
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await?;
    stored_passkeys_from_rows(rows)
}

fn stored_passkeys_from_rows(
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<StoredPasskey>, ApiError> {
    rows.into_iter()
        .map(|row| {
            let credential_json: String = row.try_get("credential_json")?;
            let credential = serde_json::from_str(&credential_json)?;
            Ok(StoredPasskey {
                id: row.try_get("id")?,
                credential,
            })
        })
        .collect()
}

async fn load_auth_user_by_username(
    state: &AppState,
    username: &str,
) -> Result<AuthUser, ApiError> {
    let row = sqlx::query(
        r#"
        select id, username, display_name, email, role, auth_provider, is_active
        from users
        where username = $1
        "#,
    )
    .bind(username)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(ApiError::unauthorized("User no longer exists"));
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

fn auth_token_for_user(state: &AppState, user: AuthUser) -> anyhow::Result<AuthToken> {
    Ok(AuthToken {
        access_token: create_access_token(
            &user.username,
            &state.settings.secret_key,
            state.settings.token_ttl_seconds,
        )?,
        token_type: "bearer",
        expires_in: state.settings.token_ttl_seconds,
        user,
    })
}

impl From<User> for AuthUser {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            display_name: user.display_name,
            email: user.email,
            role: user.role,
            auth_provider: user.auth_provider,
        }
    }
}

async fn upsert_federated_user(
    state: &AppState,
    username: &str,
    display_name: &str,
    email: &str,
    role: &str,
    provider: &str,
    department: Option<&str>,
) -> Result<User, ApiError> {
    Ok(sqlx::query_as::<_, User>(
        r#"
        insert into users (username, display_name, email, role, auth_provider, department)
        values ($1, $2, $3, $4, $5, $6)
        on conflict (username) do update set
            display_name = excluded.display_name,
            email = excluded.email,
        role = case when users.role in ('system_admin', 'super_admin') then users.role else excluded.role end,
            auth_provider = excluded.auth_provider,
            department = excluded.department,
            updated_at = now()
        returning id, username, display_name, email, role, auth_provider, department, is_active, created_at
        "#,
    )
    .bind(username)
    .bind(display_name)
    .bind(email)
    .bind(role)
    .bind(provider)
    .bind(department)
    .fetch_one(&state.pool)
    .await?)
}

fn validate_federated_role(role: &str) -> Result<(), ApiError> {
    match role {
        ROLE_LAB_MEMBER | ROLE_VISITOR => Ok(()),
        _ => Err(ApiError::bad_request(
            "Federated login role must be lab_member or visitor",
        )),
    }
}

fn federated_signature_message(
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

fn required_federated_param(value: Option<String>, field: &str) -> Result<String, ApiError> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request(format!("Missing federated login field: {field}")))
}

fn safe_local_redirect(value: Option<&str>) -> Result<String, ApiError> {
    match value {
        Some(value) if value.starts_with('/') && !value.starts_with("//") => Ok(value.to_string()),
        Some(_) => Err(ApiError::bad_request("Redirect must be a local path")),
        None => Ok("/".to_string()),
    }
}

async fn auth_me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AuthUser>, ApiError> {
    Ok(Json(require_user(&state, &headers).await?))
}

async fn my_labs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<LabMembership>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let memberships = if is_system_admin(&actor) {
        sqlx::query_as::<_, LabMembership>(
            r#"
            select id as lab_id, name as lab_name, 'system_admin'::text as role
            from labs
            order by name asc
            "#,
        )
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, LabMembership>(
            r#"
            select lu.lab_id, l.name as lab_name, lu.lab_role as role
            from lab_users lu
            join labs l on lu.lab_id = l.id
            where lu.user_id = $1
            order by l.name asc
            "#,
        )
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(memberships))
}

async fn require_user(state: &AppState, headers: &HeaderMap) -> Result<AuthUser, ApiError> {
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

fn is_admin(user: &AuthUser) -> bool {
    is_system_admin(user)
}

fn is_system_admin(user: &AuthUser) -> bool {
    matches!(user.role.as_str(), ROLE_SYSTEM_ADMIN | "super_admin")
}

fn require_admin(user: &AuthUser) -> Result<(), ApiError> {
    if is_system_admin(user) {
        Ok(())
    } else {
        Err(ApiError::forbidden("System administrator role required"))
    }
}

fn validate_global_role(role: &str) -> Result<(), ApiError> {
    if matches!(role, ROLE_LAB_MEMBER | ROLE_VISITOR) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "API user role must be lab_member or visitor",
        ))
    }
}

fn validate_lab_role(role: &str) -> Result<(), ApiError> {
    if matches!(role, ROLE_LAB_ADMIN | ROLE_LAB_MEMBER | ROLE_VISITOR) {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Lab role must be lab_admin, lab_member, or visitor",
        ))
    }
}

async fn is_lab_admin(pool: &PgPool, lab_id: i64, user_id: i64) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "select exists(select 1 from lab_users where lab_id = $1 and user_id = $2 and lab_role = 'lab_admin')",
    )
    .bind(lab_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

async fn require_lab_manager(pool: &PgPool, actor: &AuthUser, lab_id: i64) -> Result<(), ApiError> {
    if is_system_admin(actor) || is_lab_admin(pool, lab_id, actor.id).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "System administrator or lab administrator role required",
        ))
    }
}

async fn require_lab_access(pool: &PgPool, actor: &AuthUser, lab_id: i64) -> Result<(), ApiError> {
    if is_system_admin(actor) {
        return Ok(());
    }
    let exists = sqlx::query_scalar::<_, bool>(
        "select exists(select 1 from lab_users where lab_id = $1 and user_id = $2)",
    )
    .bind(lab_id)
    .bind(actor.id)
    .fetch_one(pool)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::forbidden("Lab access required"))
    }
}

async fn lab_role_for_user(
    pool: &PgPool,
    lab_id: i64,
    user_id: i64,
) -> Result<Option<String>, ApiError> {
    Ok(sqlx::query_scalar::<_, String>(
        "select lab_role from lab_users where lab_id = $1 and user_id = $2",
    )
    .bind(lab_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

async fn require_lab_role(
    pool: &PgPool,
    actor: &AuthUser,
    lab_id: i64,
    allowed_roles: &[&str],
) -> Result<(), ApiError> {
    if is_system_admin(actor) {
        return Ok(());
    }
    let role = lab_role_for_user(pool, lab_id, actor.id).await?;
    if role
        .as_deref()
        .is_some_and(|role| allowed_roles.contains(&role))
    {
        Ok(())
    } else {
        Err(ApiError::forbidden("Insufficient lab role"))
    }
}

async fn hazard_scope(
    pool: &PgPool,
    hazard_id: i64,
) -> Result<(Option<i64>, i64, Option<i64>), ApiError> {
    let row = sqlx::query(
        "select lab_id, reported_by, responsible_user_id from safety_hazards where id = $1",
    )
    .bind(hazard_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Hazard not found"))?;
    Ok((
        row.try_get("lab_id")?,
        row.try_get("reported_by")?,
        row.try_get("responsible_user_id")?,
    ))
}

async fn equipment_lab_id(pool: &PgPool, equipment_id: i64) -> Result<Option<i64>, ApiError> {
    let lab_id = sqlx::query_scalar::<_, Option<i64>>("select lab_id from equipment where id = $1")
        .bind(equipment_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Equipment not found"))?;
    Ok(lab_id)
}

fn ensure_self_or_admin(user: &AuthUser, target_user_id: i64) -> Result<(), ApiError> {
    if is_system_admin(user) || user.id == target_user_id {
        Ok(())
    } else {
        Err(ApiError::forbidden("Cannot manage another user's record"))
    }
}

fn validate_lab_status(status: &str) -> Result<(), ApiError> {
    if matches!(status, "active" | "inactive" | "maintenance") {
        Ok(())
    } else {
        Err(ApiError::bad_request(
            "Lab status must be active, inactive, or maintenance",
        ))
    }
}

async fn resolve_lab_reference(
    pool: &PgPool,
    lab_id: Option<i64>,
    lab_name: Option<String>,
) -> Result<(Option<i64>, String), ApiError> {
    if let Some(lab_id) = lab_id {
        let name = sqlx::query_scalar::<_, String>("select name from labs where id = $1")
            .bind(lab_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| ApiError::not_found("Lab not found"))?;
        return Ok((Some(lab_id), name));
    }

    let lab_name = lab_name
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("lab_id or lab_name is required"))?;
    Ok((None, lab_name))
}

async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserCreate>,
) -> Result<Json<User>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    if let Some(password) = payload.password.as_deref() {
        validate_password_strength(password).map_err(ApiError::bad_request)?;
    }
    let auth_provider = payload.auth_provider.unwrap_or_else(|| "password".into());
    if !matches!(auth_provider.as_str(), "password" | "sso" | "oauth") {
        return Err(ApiError::bad_request(
            "auth_provider must be password, sso, or oauth",
        ));
    }
    if auth_provider == "password" && payload.password.is_none() {
        return Err(ApiError::bad_request("Password users require a password"));
    }
    let role = payload.role.unwrap_or_else(|| ROLE_LAB_MEMBER.to_string());
    validate_global_role(&role)?;
    let password_hash = payload.password.as_deref().map(hash_password);
    let user = sqlx::query_as::<_, User>(
        r#"
        insert into users (username, display_name, email, role, auth_provider, department, password_hash)
        values ($1, $2, $3, $4, $5, $6, $7)
        returning id, username, display_name, email, role, auth_provider, department, is_active, created_at
        "#,
    )
    .bind(payload.username)
    .bind(payload.display_name)
    .bind(payload.email)
    .bind(role)
    .bind(auth_provider)
    .bind(payload.department)
    .bind(password_hash)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(user))
}

async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<User>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let is_admin = is_system_admin(&actor) || {
        let has_admin_membership = sqlx::query_scalar::<_, bool>(
            "select exists(select 1 from lab_users where user_id = $1 and lab_role = 'lab_admin')",
        )
        .bind(actor.id)
        .fetch_one(&state.pool)
        .await
        .unwrap_or(false);
        has_admin_membership
    };
    if !is_admin {
        return Err(ApiError::forbidden(
            "System administrator or laboratory administrator role required",
        ));
    }
    let q = wildcard(query.q);
    let users = sqlx::query_as::<_, User>(
        r#"
        select id, username, display_name, email, role, auth_provider, department, is_active, created_at
        from users
        where ($1::text is null or username ilike $1 or display_name ilike $1 or email ilike $1)
          and ($2::text is null or role = $2)
        order by created_at desc
        limit $3 offset $4
        "#,
    )
    .bind(q)
    .bind(query.role)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users))
}

async fn create_lab(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<LabCreate>,
) -> Result<Json<Lab>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    validate_lab_status(payload.status.as_deref().unwrap_or("active"))?;
    let lab = sqlx::query_as::<_, Lab>(
        r#"
        insert into labs (code, name, location, department, manager_user_id, contact, status, description)
        values ($1, $2, $3, $4, $5, $6, $7, $8)
        returning id, code, name, location, department, manager_user_id, contact, status, description, created_at
        "#,
    )
    .bind(payload.code)
    .bind(payload.name)
    .bind(payload.location)
    .bind(payload.department)
    .bind(payload.manager_user_id)
    .bind(payload.contact)
    .bind(payload.status.unwrap_or_else(|| "active".to_string()))
    .bind(payload.description)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(lab))
}

async fn list_labs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Lab>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let labs = sqlx::query_as::<_, Lab>(
        r#"
        select id, code, name, location, department, manager_user_id, contact, status, description, created_at
        from labs
        where ($1::text is null or code ilike $1 or name ilike $1 or location ilike $1 or department ilike $1)
          and ($2::text is null or status = $2)
          and (
            $3::boolean
            or exists(select 1 from lab_users where lab_users.lab_id = labs.id and lab_users.user_id = $4)
          )
        order by name asc, id asc
        limit $5 offset $6
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(labs))
}

async fn get_lab(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Lab>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_access(&state.pool, &actor, id).await?;
    let lab = sqlx::query_as::<_, Lab>(
        r#"
        select id, code, name, location, department, manager_user_id, contact, status, description, created_at
        from labs
        where id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    lab.map(Json)
        .ok_or_else(|| ApiError::not_found("Lab not found"))
}

async fn update_lab(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<LabUpdate>,
) -> Result<Json<Lab>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_manager(&state.pool, &actor, id).await?;
    if let Some(status) = payload.status.as_deref() {
        validate_lab_status(status)?;
    }
    let lab = sqlx::query_as::<_, Lab>(
        r#"
        update labs
        set code = coalesce($1, code),
            name = coalesce($2, name),
            location = coalesce($3, location),
            department = coalesce($4, department),
            manager_user_id = coalesce($5, manager_user_id),
            contact = coalesce($6, contact),
            status = coalesce($7, status),
            description = coalesce($8, description),
            updated_at = now()
        where id = $9
        returning id, code, name, location, department, manager_user_id, contact, status, description, created_at
        "#,
    )
    .bind(payload.code)
    .bind(payload.name)
    .bind(payload.location)
    .bind(payload.department)
    .bind(payload.manager_user_id)
    .bind(payload.contact)
    .bind(payload.status)
    .bind(payload.description)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    lab.map(Json)
        .ok_or_else(|| ApiError::not_found("Lab not found"))
}

async fn list_lab_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Vec<LabUser>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_access(&state.pool, &actor, id).await?;
    let users = sqlx::query_as::<_, LabUser>(
        r#"
        select
          lu.id, lu.lab_id, lu.user_id, lu.lab_role, lu.created_at,
          u.username, u.display_name, u.email, u.role as global_role
        from lab_users lu
        join users u on lu.user_id = u.id
        where lu.lab_id = $1
        order by
          case lu.lab_role
            when 'lab_admin' then 1
            when 'lab_member' then 2
            when 'visitor' then 3
            else 4
          end,
          lu.user_id asc
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users))
}

async fn assign_lab_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<LabUserAssign>,
) -> Result<Json<LabUser>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_manager(&state.pool, &actor, id).await?;
    validate_lab_role(&payload.lab_role)?;
    let assigned = sqlx::query_as::<_, LabUser>(
        r#"
        with inserted as (
          insert into lab_users (lab_id, user_id, lab_role)
          values ($1, $2, $3)
          on conflict (lab_id, user_id) do update set
            lab_role = excluded.lab_role,
            updated_at = now()
          returning id, lab_id, user_id, lab_role, created_at
        )
        select
          i.id, i.lab_id, i.user_id, i.lab_role, i.created_at,
          u.username, u.display_name, u.email, u.role as global_role
        from inserted i
        join users u on i.user_id = u.id
        "#,
    )
    .bind(id)
    .bind(payload.user_id)
    .bind(payload.lab_role)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(assigned))
}

async fn remove_lab_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((id, user_id)): AxumPath<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_manager(&state.pool, &actor, id).await?;
    let deleted = sqlx::query(
        r#"
        delete from lab_users
        where lab_id = $1 and user_id = $2
        "#,
    )
    .bind(id)
    .bind(user_id)
    .execute(&state.pool)
    .await?;
    if deleted.rows_affected() == 0 {
        Err(ApiError::not_found("Lab user not found"))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}

async fn create_regulation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RegulationCreate>,
) -> Result<Json<Regulation>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    Ok(Json(sqlx::query_as::<_, Regulation>(
        r#"
        insert into regulations (title, regulation_type, issuing_authority, effective_date, summary, file_url)
        values ($1, $2, $3, $4, $5, $6)
        returning id, title, regulation_type, issuing_authority, effective_date, summary, file_url, created_at
        "#,
    )
    .bind(payload.title)
    .bind(payload.regulation_type)
    .bind(payload.issuing_authority)
    .bind(payload.effective_date)
    .bind(payload.summary)
    .bind(payload.file_url)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_regulations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Regulation>>, ApiError> {
    require_user(&state, &headers).await?;
    let rows = sqlx::query_as::<_, Regulation>(
        r#"
        select id, title, regulation_type, issuing_authority, effective_date, summary, file_url, created_at
        from regulations
        where ($1::text is null or title ilike $1)
        order by created_at desc
        limit $2 offset $3
        "#,
    )
    .bind(wildcard(query.q))
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_incident(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<IncidentCaseCreate>,
) -> Result<Json<IncidentCase>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, lab_name) =
        resolve_lab_reference(&state.pool, payload.lab_id, payload.lab_name).await?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    Ok(Json(sqlx::query_as::<_, IncidentCase>(
        r#"
        insert into incident_cases (title, lab_id, lab_name, occurred_on, severity, category, root_cause, corrective_actions, file_url)
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        returning id, lab_id, title, lab_name, occurred_on, severity, category, root_cause, corrective_actions, file_url, created_at
        "#,
    )
    .bind(payload.title)
    .bind(lab_id)
    .bind(lab_name)
    .bind(payload.occurred_on)
    .bind(payload.severity)
    .bind(payload.category)
    .bind(payload.root_cause)
    .bind(payload.corrective_actions)
    .bind(payload.file_url)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_incidents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<IncidentCase>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = sqlx::query_as::<_, IncidentCase>(
        r#"
        select id, lab_id, title, lab_name, occurred_on, severity, category, root_cause, corrective_actions, file_url, created_at
        from incident_cases
        where ($1::text is null or title ilike $1)
          and (
            $2::boolean
            or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $3)
          )
          and ($4::bigint is null or lab_id = $4)
        order by occurred_on desc
        limit $5 offset $6
        "#,
    )
    .bind(wildcard(query.q))
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_training(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<TrainingCreate>,
) -> Result<Json<Training>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    Ok(Json(
        sqlx::query_as::<_, Training>(
            r#"
        insert into trainings (title, target_role, status, starts_on, exam_required_score)
        values ($1, $2, $3, $4, $5)
        returning id, title, target_role, status, starts_on, exam_required_score, created_at
        "#,
        )
        .bind(payload.title)
        .bind(payload.target_role)
        .bind(payload.status.unwrap_or_else(|| "draft".into()))
        .bind(payload.starts_on)
        .bind(payload.exam_required_score.unwrap_or(80))
        .fetch_one(&state.pool)
        .await?,
    ))
}

async fn list_trainings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Training>>, ApiError> {
    require_user(&state, &headers).await?;
    let rows = sqlx::query_as::<_, Training>(
        "select id, title, target_role, status, starts_on, exam_required_score, created_at from trainings where ($1::text is null or status = $1) order by created_at desc limit $2 offset $3",
    )
    .bind(query.status)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_exam_result(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ExamResultCreate>,
) -> Result<Json<ExamResult>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.user_id)?;
    Ok(Json(sqlx::query_as::<_, ExamResult>(
        "insert into exam_results (training_id, user_id, score, status) values ($1, $2, $3, $4) returning id, training_id, user_id, score, status, created_at",
    )
    .bind(payload.training_id)
    .bind(payload.user_id)
    .bind(payload.score)
    .bind(payload.status)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_exam_results(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ExamResult>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let rows = if is_admin(&actor) {
        sqlx::query_as::<_, ExamResult>(
            "select id, training_id, user_id, score, status, created_at from exam_results order by created_at desc",
        )
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, ExamResult>(
            "select id, training_id, user_id, score, status, created_at from exam_results where user_id = $1 order by created_at desc",
        )
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

async fn create_equipment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<EquipmentCreate>,
) -> Result<Json<Equipment>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, lab_name) =
        resolve_lab_reference(&state.pool, payload.lab_id, payload.lab_name).await?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    Ok(Json(
        sqlx::query_as::<_, Equipment>(
            r#"
        insert into equipment (asset_code, name, lab_id, lab_name, status, owner)
        values ($1, $2, $3, $4, $5, $6)
        returning id, lab_id, asset_code, name, lab_name, status, owner, created_at
        "#,
        )
        .bind(payload.asset_code)
        .bind(payload.name)
        .bind(lab_id)
        .bind(lab_name)
        .bind(payload.status.unwrap_or_else(|| "available".into()))
        .bind(payload.owner)
        .fetch_one(&state.pool)
        .await?,
    ))
}

async fn list_equipment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Equipment>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = sqlx::query_as::<_, Equipment>(
        r#"
        select id, lab_id, asset_code, name, lab_name, status, owner, created_at
        from equipment
        where ($1::text is null or name ilike $1 or asset_code ilike $1)
          and ($2::text is null or status = $2)
          and (
            $3::boolean
            or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $4)
          )
          and ($5::bigint is null or lab_id = $5)
        order by created_at desc
        limit $6 offset $7
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_booking(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<EquipmentBookingCreate>,
) -> Result<Json<EquipmentBooking>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.user_id)?;
    if let Some(lab_id) = equipment_lab_id(&state.pool, payload.equipment_id).await? {
        require_lab_role(
            &state.pool,
            &actor,
            lab_id,
            &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
        )
        .await?;
    } else if !is_system_admin(&actor) {
        return Err(ApiError::forbidden("Equipment lab access required"));
    }
    if payload.ends_at <= payload.starts_at {
        return Err(ApiError::bad_request(
            "Booking end time must be later than start time",
        ));
    }
    let conflict: Option<(i64,)> = sqlx::query_as(
        "select id from equipment_bookings where equipment_id = $1 and starts_at < $2 and ends_at > $3 limit 1",
    )
    .bind(payload.equipment_id)
    .bind(payload.ends_at)
    .bind(payload.starts_at)
    .fetch_optional(&state.pool)
    .await?;
    if conflict.is_some() {
        return Err(ApiError::conflict(
            "Equipment is already booked for the selected time range",
        ));
    }
    Ok(Json(sqlx::query_as::<_, EquipmentBooking>(
        "insert into equipment_bookings (equipment_id, user_id, starts_at, ends_at, purpose) values ($1, $2, $3, $4, $5) returning id, equipment_id, user_id, starts_at, ends_at, purpose, created_at",
    )
    .bind(payload.equipment_id)
    .bind(payload.user_id)
    .bind(payload.starts_at)
    .bind(payload.ends_at)
    .bind(payload.purpose)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_bookings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<EquipmentBooking>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = if is_admin(&actor) {
        sqlx::query_as::<_, EquipmentBooking>(
            r#"
            select equipment_bookings.id, equipment_id, user_id, starts_at, ends_at, purpose, equipment_bookings.created_at
            from equipment_bookings
            join equipment on equipment.id = equipment_bookings.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
            order by starts_at desc
            "#,
        )
        .bind(query.lab_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, EquipmentBooking>(
            r#"
            select equipment_bookings.id, equipment_id, user_id, starts_at, ends_at, purpose, equipment_bookings.created_at
            from equipment_bookings
            join equipment on equipment.id = equipment_bookings.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
              and (
                user_id = $2
                or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2)
              )
            order by starts_at desc
            "#,
        )
        .bind(query.lab_id)
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

async fn create_repair(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RepairTicketCreate>,
) -> Result<Json<RepairTicket>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.reported_by)?;
    if let Some(lab_id) = equipment_lab_id(&state.pool, payload.equipment_id).await? {
        require_lab_role(
            &state.pool,
            &actor,
            lab_id,
            &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
        )
        .await?;
    } else if !is_system_admin(&actor) {
        return Err(ApiError::forbidden("Equipment lab access required"));
    }
    Ok(Json(sqlx::query_as::<_, RepairTicket>(
        "insert into repair_tickets (equipment_id, reported_by, description, status) values ($1, $2, $3, $4) returning id, equipment_id, reported_by, description, status, created_at",
    )
    .bind(payload.equipment_id)
    .bind(payload.reported_by)
    .bind(payload.description)
    .bind(payload.status.unwrap_or_else(|| "open".into()))
    .fetch_one(&state.pool)
    .await?))
}

async fn list_repairs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<RepairTicket>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = if is_admin(&actor) {
        sqlx::query_as::<_, RepairTicket>(
            r#"
            select repair_tickets.id, repair_tickets.equipment_id, reported_by, description, repair_tickets.status, repair_tickets.created_at
            from repair_tickets
            join equipment on equipment.id = repair_tickets.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
            order by repair_tickets.created_at desc
            "#,
        )
        .bind(query.lab_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, RepairTicket>(
            r#"
            select repair_tickets.id, repair_tickets.equipment_id, reported_by, description, repair_tickets.status, repair_tickets.created_at
            from repair_tickets
            join equipment on equipment.id = repair_tickets.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
              and (
                reported_by = $2
                or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2)
              )
            order by repair_tickets.created_at desc
            "#,
        )
        .bind(query.lab_id)
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

async fn update_repair(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<RepairTicketUpdate>,
) -> Result<Json<RepairTicket>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let lab_id = sqlx::query_scalar::<_, Option<i64>>(
        r#"
        select equipment.lab_id
        from repair_tickets
        join equipment on equipment.id = repair_tickets.equipment_id
        where repair_tickets.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Repair ticket not found"))?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    let row = sqlx::query_as::<_, RepairTicket>(
        "update repair_tickets set status = $1, updated_at = now() where id = $2 returning id, equipment_id, reported_by, description, status, created_at",
    )
    .bind(payload.status)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Repair ticket not found"))
}

async fn create_hazard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SafetyHazardCreate>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.reported_by)?;
    let (lab_id, lab_name) =
        resolve_lab_reference(&state.pool, payload.lab_id, payload.lab_name).await?;
    if let Some(lab_id) = lab_id {
        require_lab_role(
            &state.pool,
            &actor,
            lab_id,
            &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
        )
        .await?;
    } else if !is_system_admin(&actor) {
        return Err(ApiError::bad_request("lab_id is required"));
    }
    Ok(Json(sqlx::query_as::<_, SafetyHazard>(
        r#"
        insert into safety_hazards (title, lab_id, lab_name, category, description, reported_by, issue_photo_url)
        values ($1, $2, $3, $4, $5, $6, $7)
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.title)
    .bind(lab_id)
    .bind(lab_name)
    .bind(payload.category)
    .bind(payload.description)
    .bind(payload.reported_by)
    .bind(payload.issue_photo_url)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_hazards(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<SafetyHazard>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = sqlx::query_as::<_, SafetyHazard>(
        r#"
        select id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        from safety_hazards
        where ($1::text is null or title ilike $1 or description ilike $1)
          and ($2::text is null or status = $2)
          and ($3::bigint is null or responsible_user_id = $3)
          and ($4::bigint is null or reported_by = $4)
          and (
            $5::boolean
            or reported_by = $6
            or responsible_user_id = $6
            or exists(select 1 from lab_users where lab_users.lab_id = safety_hazards.lab_id and lab_users.user_id = $6)
          )
          and ($7::bigint is null or lab_id = $7)
        order by created_at desc
        limit $8 offset $9
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(query.responsible_user_id)
    .bind(query.reported_by)
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn claim_hazard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardClaim>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, _, _) = hazard_scope(&state.pool, id).await?;
    if is_system_admin(&actor) {
        // Global system administrators may assign any responsible user.
    } else if let Some(lab_id) = lab_id {
        if is_lab_admin(&state.pool, lab_id, actor.id).await? {
            require_lab_access(&state.pool, &actor, lab_id).await?;
        } else {
            ensure_self_or_admin(&actor, payload.responsible_user_id)?;
            require_lab_role(
                &state.pool,
                &actor,
                lab_id,
                &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
            )
            .await?;
        }
    } else {
        ensure_self_or_admin(&actor, payload.responsible_user_id)?;
    }
    let row = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards set responsible_user_id = $1, status = 'claimed', updated_at = now()
        where id = $2
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.responsible_user_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Hazard not found"))
}

async fn remediate_hazard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardRemediation>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, _, responsible_user_id) = hazard_scope(&state.pool, id).await?;
    if let Some(lab_id) = lab_id {
        if !is_system_admin(&actor) && !is_lab_admin(&state.pool, lab_id, actor.id).await? {
            if responsible_user_id != Some(actor.id) {
                return Err(ApiError::forbidden(
                    "Cannot remediate another user's hazard",
                ));
            }
            require_lab_role(
                &state.pool,
                &actor,
                lab_id,
                &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
            )
            .await?;
        }
    }
    let row = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards
        set remediation_photo_url = $1, remediation_note = $2, status = 'remediation_submitted', updated_at = now()
        where id = $3 and responsible_user_id is not null
          and ($4::boolean or responsible_user_id = $5)
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.remediation_photo_url)
    .bind(payload.remediation_note)
    .bind(id)
    .bind(is_admin(&actor))
    .bind(actor.id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::bad_request("Hazard must exist and be claimed before remediation"))
}

async fn update_hazard_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardStatusUpdate>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, _, _) = hazard_scope(&state.pool, id).await?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    let row = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards set status = $1, updated_at = now()
        where id = $2
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.status)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Hazard not found"))
}

async fn dashboard_stats(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<DashboardStats>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let total: i64 = sqlx::query("select count(*)::bigint as count from exam_results")
        .fetch_one(&state.pool)
        .await?
        .get("count");
    let passed: i64 =
        sqlx::query("select count(*)::bigint as count from exam_results where status = 'passed'")
            .fetch_one(&state.pool)
            .await?
            .get("count");
    let incident_count: i64 = sqlx::query(
        r#"
        select count(*)::bigint as count
        from incident_cases
        where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $2))
          and ($3::bigint is null or lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?
    .get("count");
    let equipment_count: i64 = sqlx::query(
        r#"
        select count(*)::bigint as count
        from equipment
        where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2))
          and ($3::bigint is null or lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?
    .get("count");
    let open_repairs: i64 = sqlx::query(
        r#"
        select count(*)::bigint as count
        from repair_tickets
        join equipment on equipment.id = repair_tickets.equipment_id
        where repair_tickets.status = 'open'
          and ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2))
          and ($3::bigint is null or equipment.lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?
    .get("count");
    Ok(Json(DashboardStats {
        regulation_count: table_count(&state.pool, "regulations").await?,
        incident_count,
        training_count: table_count(&state.pool, "trainings").await?,
        equipment_count,
        open_repair_count: open_repairs,
        exam_pass_rate: if total == 0 {
            0.0
        } else {
            passed as f64 / total as f64
        },
    }))
}

async fn incident_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<IncidentAnalytics>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    Ok(Json(IncidentAnalytics {
        by_category: count_incident_buckets(&state.pool, "category", &actor, query.lab_id).await?,
        by_severity: count_incident_buckets(&state.pool, "severity", &actor, query.lab_id).await?,
    }))
}

async fn regulation_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<RegulationAnalytics>, ApiError> {
    require_user(&state, &headers).await?;
    Ok(Json(RegulationAnalytics {
        by_type: count_buckets(
            &state.pool,
            "select regulation_type as name, count(*)::bigint as count from regulations group by regulation_type order by count desc",
        )
        .await?,
        by_authority: count_buckets(
            &state.pool,
            "select issuing_authority as name, count(*)::bigint as count from regulations group by issuing_authority order by count desc",
        )
        .await?,
    }))
}

async fn hazard_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<HazardAnalytics>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    Ok(Json(HazardAnalytics {
        by_status: count_hazard_buckets(&state.pool, "status", &actor, query.lab_id).await?,
        by_category: count_hazard_buckets(&state.pool, "category", &actor, query.lab_id).await?,
    }))
}

async fn upload_regulation_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    save_upload(&state, multipart, "regulations")
        .await
        .map(Json)
}

async fn upload_incident_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    save_upload(&state, multipart, "incidents").await.map(Json)
}

async fn upload_hazard_issue_photo(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    require_user(&state, &headers).await?;
    save_upload(&state, multipart, "hazards/issue")
        .await
        .map(Json)
}

async fn upload_hazard_remediation_photo(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    require_user(&state, &headers).await?;
    save_upload(&state, multipart, "hazards/remediation")
        .await
        .map(Json)
}

async fn save_upload(
    state: &AppState,
    mut multipart: Multipart,
    category: &str,
) -> Result<UploadedFile, ApiError> {
    let Some(field) = multipart.next_field().await? else {
        return Err(ApiError::bad_request("file is required"));
    };
    let filename = field.file_name().unwrap_or("upload.bin").to_string();
    let content_type = field.content_type().map(ToString::to_string);
    let bytes = field.bytes().await?;
    let safe_name = Path::new(&filename)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("upload.bin");
    let stored_name = format!("{}-{safe_name}", Uuid::new_v4());
    let target_dir = state.settings.upload_dir.join(category);
    fs::create_dir_all(&target_dir).await?;
    fs::write(target_dir.join(&stored_name), &bytes).await?;
    Ok(UploadedFile {
        filename,
        content_type,
        size: bytes.len(),
        url: format!("/uploads/{category}/{stored_name}"),
    })
}

async fn count_buckets(pool: &PgPool, sql: &str) -> Result<Vec<CountBucket>, ApiError> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| CountBucket {
            name: row.get("name"),
            count: row.get("count"),
        })
        .collect())
}

async fn count_incident_buckets(
    pool: &PgPool,
    column: &'static str,
    actor: &AuthUser,
    lab_id: Option<i64>,
) -> Result<Vec<CountBucket>, ApiError> {
    let sql = match column {
        "category" => {
            r#"
            select category as name, count(*)::bigint as count
            from incident_cases
            where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $2))
              and ($3::bigint is null or lab_id = $3)
            group by category
            order by count desc
            "#
        }
        "severity" => {
            r#"
            select severity as name, count(*)::bigint as count
            from incident_cases
            where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $2))
              and ($3::bigint is null or lab_id = $3)
            group by severity
            order by count desc
            "#
        }
        _ => return Err(ApiError::bad_request("Unsupported analytics column")),
    };
    let rows = sqlx::query(sql)
        .bind(is_system_admin(actor))
        .bind(actor.id)
        .bind(lab_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| CountBucket {
            name: row.get("name"),
            count: row.get("count"),
        })
        .collect())
}

async fn count_hazard_buckets(
    pool: &PgPool,
    column: &'static str,
    actor: &AuthUser,
    lab_id: Option<i64>,
) -> Result<Vec<CountBucket>, ApiError> {
    let sql = match column {
        "status" => {
            r#"
            select status as name, count(*)::bigint as count
            from safety_hazards
            where (
                $1::boolean
                or reported_by = $2
                or responsible_user_id = $2
                or exists(select 1 from lab_users where lab_users.lab_id = safety_hazards.lab_id and lab_users.user_id = $2)
              )
              and ($3::bigint is null or lab_id = $3)
            group by status
            order by count desc
            "#
        }
        "category" => {
            r#"
            select category as name, count(*)::bigint as count
            from safety_hazards
            where (
                $1::boolean
                or reported_by = $2
                or responsible_user_id = $2
                or exists(select 1 from lab_users where lab_users.lab_id = safety_hazards.lab_id and lab_users.user_id = $2)
              )
              and ($3::bigint is null or lab_id = $3)
            group by category
            order by count desc
            "#
        }
        _ => return Err(ApiError::bad_request("Unsupported analytics column")),
    };
    let rows = sqlx::query(sql)
        .bind(is_system_admin(actor))
        .bind(actor.id)
        .bind(lab_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| CountBucket {
            name: row.get("name"),
            count: row.get("count"),
        })
        .collect())
}

async fn table_count(pool: &PgPool, table: &'static str) -> Result<i64, ApiError> {
    let sql = format!("select count(*)::bigint as count from {table}");
    Ok(sqlx::query(&sql).fetch_one(pool).await?.get("count"))
}

fn wildcard(q: Option<String>) -> Option<String> {
    q.filter(|value| !value.trim().is_empty())
        .map(|value| format!("%{}%", value.trim()))
}

fn limit(value: Option<i64>) -> i64 {
    value.unwrap_or(50).clamp(1, 100)
}

fn offset(value: Option<i64>) -> i64 {
    value.unwrap_or(0).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, header},
    };
    use sqlx::{Executor, postgres::PgPoolOptions};
    use tower::ServiceExt;

    use crate::{db, security::hash_password};

    struct TestApp {
        app: Router,
        schema: String,
        admin_token: String,
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
            pool,
            settings,
            passkey_registrations: Mutex::new(HashMap::new()),
            passkey_authentications: Mutex::new(HashMap::new()),
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
            schema,
            admin_token,
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
        let body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: text/plain\r\n\r\n{content}\r\n--{boundary}--\r\n"
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

    #[tokio::test]
    async fn backend_safety_management_flow_is_enforced() -> anyhow::Result<()> {
        let Some(ctx) = test_app().await? else {
            return Ok(());
        };

        let (status, login) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/auth/password-login",
            None,
            serde_json::json!({
                "username": "admin",
                "password": "AdminStrong123!"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(login["user"]["role"], "system_admin");

        let (status, _) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/regulations",
            Some(&ctx.researcher_token),
            serde_json::json!({
                "title": "No permission",
                "regulation_type": "internal",
                "issuing_authority": "Lab",
                "effective_date": "2026-01-01",
                "summary": "researcher cannot create regulations",
                "file_url": null
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);

        let managed_username = format!("managed_{}", ctx.schema);
        let (status, _) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/users",
            Some(&ctx.admin_token),
            serde_json::json!({
                "username": managed_username,
                "display_name": "Managed Researcher",
                "email": format!("{}@example.com", ctx.schema),
                "role": "lab_member",
                "auth_provider": "password",
                "department": "公共实验平台",
                "password": "weak"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);

        let (status, managed_user) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/users",
            Some(&ctx.admin_token),
            serde_json::json!({
                "username": managed_username,
                "display_name": "Managed Researcher",
                "email": format!("{}@example.com", ctx.schema),
                "role": "lab_member",
                "auth_provider": "password",
                "department": "公共实验平台",
                "password": "ManagedStrong123!"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(managed_user["role"], "lab_member");

        let (status, users) = request(
            &ctx.app,
            Method::GET,
            "/api/v1/users?role=lab_member",
            Some(&ctx.admin_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert!(users.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|user| user["username"] == managed_user["username"])
        }));

        let (status, regulation_upload) = upload(
            &ctx.app,
            "/api/v1/regulations/upload",
            &ctx.admin_token,
            "regulation.txt",
            "wear goggles",
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert!(
            regulation_upload["url"]
                .as_str()
                .is_some_and(|url| url.starts_with("/uploads/regulations/"))
        );

        let (status, regulation) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/regulations",
            Some(&ctx.admin_token),
            serde_json::json!({
                "title": "危险化学品安全管理条例",
                "regulation_type": "国家法规",
                "issuing_authority": "国务院",
                "effective_date": "2026-01-01",
                "summary": "危险化学品采购、储存、使用和处置要求。",
                "file_url": regulation_upload["url"]
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(regulation["file_url"], regulation_upload["url"]);

        let (status, incident_upload) = upload(
            &ctx.app,
            "/api/v1/incidents/upload",
            &ctx.admin_token,
            "incident.txt",
            "incident attachment",
        )
        .await?;
        assert_eq!(status, StatusCode::OK);

        let (status, lab) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/labs",
            Some(&ctx.admin_token),
            serde_json::json!({
                "code": format!("LAB-{}", ctx.schema),
                "name": "有机化学实验室",
                "location": "实验楼A-302",
                "department": "化学学院",
                "manager_user_id": ctx.researcher_id,
                "contact": "lab@example.com",
                "status": "active",
                "description": "有机合成和试剂暂存实验室"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(lab["name"], "有机化学实验室");

        let (status, lab_member) = json_request(
            &ctx.app,
            Method::POST,
            &format!("/api/v1/labs/{}/users", lab["id"]),
            Some(&ctx.admin_token),
            serde_json::json!({
                "user_id": ctx.researcher_id,
                "lab_role": "lab_member"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(lab_member["lab_role"], "lab_member");

        let (status, lab_visitor) = json_request(
            &ctx.app,
            Method::POST,
            &format!("/api/v1/labs/{}/users", lab["id"]),
            Some(&ctx.admin_token),
            serde_json::json!({
                "user_id": managed_user["id"],
                "lab_role": "visitor"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(lab_visitor["lab_role"], "visitor");

        let (status, lab_users) = request(
            &ctx.app,
            Method::GET,
            &format!("/api/v1/labs/{}/users", lab["id"]),
            Some(&ctx.admin_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert!(lab_users.as_array().is_some_and(|items| {
            items
                .iter()
                .any(|item| item["user_id"] == managed_user["id"] && item["lab_role"] == "visitor")
        }));

        let (status, _) = request(
            &ctx.app,
            Method::DELETE,
            &format!("/api/v1/labs/{}/users/{}", lab["id"], managed_user["id"]),
            Some(&ctx.admin_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::NO_CONTENT);

        let (status, incident) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/incidents",
            Some(&ctx.admin_token),
            serde_json::json!({
                "title": "通风橱操作不当事故",
                "lab_id": lab["id"],
                "occurred_on": "2026-05-10",
                "severity": "major",
                "category": "chemical",
                "root_cause": "未按规程开启通风设备",
                "corrective_actions": "重新培训并增加班前检查",
                "file_url": incident_upload["url"]
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(incident["file_url"], incident_upload["url"]);

        let (status, training) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/trainings",
            Some(&ctx.admin_token),
            serde_json::json!({
                "title": "化学品入门安全培训",
                "target_role": "lab_member",
                "status": "published",
                "starts_on": "2026-07-01",
                "exam_required_score": 80
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);

        let (status, _) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/exam-results",
            Some(&ctx.researcher_token),
            serde_json::json!({
                "training_id": training["id"],
                "user_id": ctx.researcher_id,
                "score": 92,
                "status": "passed"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);

        let (status, equipment) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/equipment",
            Some(&ctx.admin_token),
            serde_json::json!({
                "asset_code": format!("HPLC-{}", ctx.schema),
                "name": "高效液相色谱仪",
                "lab_id": lab["id"],
                "status": "available",
                "owner": "设备管理员"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);

        let booking_payload = serde_json::json!({
            "equipment_id": equipment["id"],
            "user_id": ctx.researcher_id,
            "starts_at": "2026-07-10T02:00:00Z",
            "ends_at": "2026-07-10T04:00:00Z",
            "purpose": "样品检测"
        });
        let (status, _) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/equipment-bookings",
            Some(&ctx.researcher_token),
            booking_payload.clone(),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/equipment-bookings",
            Some(&ctx.researcher_token),
            booking_payload,
        )
        .await?;
        assert_eq!(status, StatusCode::CONFLICT);

        let (status, repair) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/repair-tickets",
            Some(&ctx.researcher_token),
            serde_json::json!({
                "equipment_id": equipment["id"],
                "reported_by": ctx.researcher_id,
                "description": "泵压异常",
                "status": "open"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        let (status, closed_repair) = json_request(
            &ctx.app,
            Method::PATCH,
            &format!("/api/v1/repair-tickets/{}", repair["id"]),
            Some(&ctx.admin_token),
            serde_json::json!({ "status": "closed" }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(closed_repair["status"], "closed");

        let (status, labs) = request(
            &ctx.app,
            Method::GET,
            "/api/v1/labs?q=有机",
            Some(&ctx.researcher_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert!(
            labs.as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == lab["id"]))
        );

        let (status, issue_photo) = upload(
            &ctx.app,
            "/api/v1/hazards/upload/issue-photo",
            &ctx.researcher_token,
            "issue.txt",
            "issue photo",
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        let (status, hazard) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/hazards",
            Some(&ctx.researcher_token),
            serde_json::json!({
                "title": "试剂柜标签缺失",
                "lab_id": lab["id"],
                "category": "chemical",
                "description": "三号试剂柜部分瓶体缺少中文标签。",
                "reported_by": ctx.researcher_id,
                "issue_photo_url": issue_photo["url"]
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);

        assert_eq!(hazard["lab_id"], lab["id"]);
        assert_eq!(hazard["lab_name"], lab["name"]);

        let (status, lab_hazards) = request(
            &ctx.app,
            Method::GET,
            &format!("/api/v1/hazards?lab_id={}", lab["id"]),
            Some(&ctx.admin_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert!(
            lab_hazards
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == hazard["id"]))
        );

        let (status, claimed) = json_request(
            &ctx.app,
            Method::POST,
            &format!("/api/v1/hazards/{}/claim", hazard["id"]),
            Some(&ctx.researcher_token),
            serde_json::json!({ "responsible_user_id": ctx.researcher_id }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(claimed["status"], "claimed");

        let (status, remediation_photo) = upload(
            &ctx.app,
            "/api/v1/hazards/upload/remediation-photo",
            &ctx.researcher_token,
            "remediation.txt",
            "fixed photo",
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        let (status, remediated) = json_request(
            &ctx.app,
            Method::POST,
            &format!("/api/v1/hazards/{}/remediation", hazard["id"]),
            Some(&ctx.researcher_token),
            serde_json::json!({
                "remediation_photo_url": remediation_photo["url"],
                "remediation_note": "已补充标签并复核。"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(remediated["status"], "remediation_submitted");

        let (status, _) = json_request(
            &ctx.app,
            Method::PATCH,
            &format!("/api/v1/hazards/{}/status", hazard["id"]),
            Some(&ctx.researcher_token),
            serde_json::json!({ "status": "closed" }),
        )
        .await?;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, closed_hazard) = json_request(
            &ctx.app,
            Method::PATCH,
            &format!("/api/v1/hazards/{}/status", hazard["id"]),
            Some(&ctx.admin_token),
            serde_json::json!({ "status": "closed" }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(closed_hazard["status"], "closed");

        let (status, lab_dashboard) = request(
            &ctx.app,
            Method::GET,
            &format!("/api/v1/analytics/dashboard?lab_id={}", lab["id"]),
            Some(&ctx.researcher_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(lab_dashboard["incident_count"], 1);
        assert_eq!(lab_dashboard["equipment_count"], 1);

        let (status, lab_incident_analytics) = request(
            &ctx.app,
            Method::GET,
            &format!("/api/v1/analytics/incidents?lab_id={}", lab["id"]),
            Some(&ctx.researcher_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert!(
            lab_incident_analytics["by_category"]
                .as_array()
                .is_some_and(|items| items
                    .iter()
                    .any(|item| item["name"] == "chemical" && item["count"] == 1))
        );

        let (status, lab_hazard_analytics) = request(
            &ctx.app,
            Method::GET,
            &format!("/api/v1/analytics/hazards?lab_id={}", lab["id"]),
            Some(&ctx.researcher_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK);
        assert!(
            lab_hazard_analytics["by_status"]
                .as_array()
                .is_some_and(|items| items
                    .iter()
                    .any(|item| item["name"] == "closed" && item["count"] == 1))
        );

        for path in [
            "/api/v1/regulations?q=危险化学品",
            "/api/v1/incidents",
            "/api/v1/trainings",
            "/api/v1/equipment",
            "/api/v1/equipment-bookings",
            "/api/v1/repair-tickets",
            "/api/v1/hazards",
        ] {
            let (status, value) = request(
                &ctx.app,
                Method::GET,
                path,
                Some(&ctx.researcher_token),
                Body::empty(),
                None,
            )
            .await?;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert!(
                value.as_array().is_some_and(|items| !items.is_empty()),
                "{path}"
            );
        }

        for path in [
            "/api/v1/analytics/dashboard",
            "/api/v1/analytics/regulations",
            "/api/v1/analytics/incidents",
            "/api/v1/analytics/hazards",
        ] {
            let (status, value) = request(
                &ctx.app,
                Method::GET,
                path,
                Some(&ctx.admin_token),
                Body::empty(),
                None,
            )
            .await?;
            assert_eq!(status, StatusCode::OK, "{path}: {value}");
        }

        Ok(())
    }
}
