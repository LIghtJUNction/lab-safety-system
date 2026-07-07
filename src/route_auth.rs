use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::Html,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sqlx::Row;
use uuid::Uuid;
use webauthn_rs::prelude::{
    CreationChallengeResponse, Passkey, RequestChallengeResponse, Uuid as WebauthnUuid,
};

use crate::route_auth_support::{
    auth_token_for_user, load_auth_user_by_username, load_passkeys_for_user,
    load_passkeys_for_username, webauthn,
};
use crate::route_permissions::is_system_admin;
use crate::route_support::*;
use crate::{
    models::*,
    routes::require_user,
    security::{create_access_token, verify_message_signature, verify_password},
};

pub(crate) async fn auth_methods(State(state): State<Arc<AppState>>) -> Json<AuthMethods> {
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

pub(crate) async fn password_login(
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
    let auth_provider: String = row.try_get("auth_provider")?;
    let active: bool = row.try_get("is_active")?;
    if auth_provider != "password"
        || !active
        || !verify_password(&payload.password, password_hash.as_deref())
    {
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
            auth_provider,
        },
    }))
}

pub(crate) async fn passkey_login_start(
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

pub(crate) async fn passkey_login_finish(
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

pub(crate) async fn passkey_register_start(
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

pub(crate) async fn passkey_register_finish(
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

pub(crate) async fn list_passkeys(
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

pub(crate) async fn sso_callback(
    State(state): State<Arc<AppState>>,
    Query(payload): Query<FederatedLoginQuery>,
) -> Result<Html<String>, ApiError> {
    federated_callback(&state, "sso", state.settings.sso_enabled, payload).await
}

pub(crate) async fn oauth_callback(
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

pub(crate) async fn auth_me(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<AuthUser>, ApiError> {
    Ok(Json(require_user(&state, &headers).await?))
}

pub(crate) async fn my_labs(
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
