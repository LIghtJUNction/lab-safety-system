use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};

use crate::route_auth::{
    auth_me, auth_methods, list_passkeys, my_labs, oauth_callback, passkey_login_finish,
    passkey_login_start, passkey_register_finish, passkey_register_start, password_login,
    sso_callback,
};
use crate::route_support::AppState;

pub fn auth_routes() -> Router<Arc<AppState>> {
    Router::new()
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
}
