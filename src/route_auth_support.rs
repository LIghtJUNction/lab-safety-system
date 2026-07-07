use sqlx::Row;
use webauthn_rs::prelude::{Passkey, Url as WebauthnUrl, Webauthn, WebauthnBuilder};

use crate::{
    config::Settings,
    models::{AuthToken, AuthUser, User},
    route_support::{ApiError, AppState, StoredPasskey},
    security::create_access_token,
};

pub(crate) fn webauthn(settings: &Settings) -> Result<Webauthn, ApiError> {
    let origin = WebauthnUrl::parse(&settings.webauthn_origin)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    WebauthnBuilder::new(&settings.webauthn_rp_id, &origin)
        .map_err(|error| ApiError::bad_request(error.to_string()))?
        .build()
        .map_err(|error| ApiError::bad_request(error.to_string()))
}

pub(crate) async fn load_passkeys_for_username(
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

pub(crate) async fn load_passkeys_for_user(
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

pub(crate) async fn load_auth_user_by_username(
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

pub(crate) fn auth_token_for_user(state: &AppState, user: AuthUser) -> anyhow::Result<AuthToken> {
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

fn stored_passkeys_from_rows(
    rows: Vec<sqlx::postgres::PgRow>,
) -> Result<Vec<StoredPasskey>, ApiError> {
    rows.into_iter()
        .map(|row| {
            let credential_json: String = row.try_get("credential_json")?;
            let credential: Passkey = serde_json::from_str(&credential_json)?;
            Ok(StoredPasskey {
                id: row.try_get("id")?,
                credential,
            })
        })
        .collect()
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
