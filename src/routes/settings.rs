use std::sync::Arc;

use axum::{Router, routing::get};

use crate::route_settings::{
    get_auth_settings, get_deployment_settings, get_login_carousel, reset_login_carousel,
    update_auth_settings, update_login_carousel,
};
use crate::route_support::AppState;

pub fn settings_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/settings/auth",
            get(get_auth_settings).patch(update_auth_settings),
        )
        .route("/api/v1/settings/deployment", get(get_deployment_settings))
        .route(
            "/api/v1/settings/login-carousel",
            get(get_login_carousel)
                .patch(update_login_carousel)
                .delete(reset_login_carousel),
        )
}
