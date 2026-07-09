use std::sync::Arc;

use axum::{
    Router,
    routing::{delete, get, patch},
};

use crate::route_support::AppState;
use crate::route_users_labs::{
    assign_lab_user, create_lab, create_user, get_lab, list_lab_users, list_labs, list_users,
    remove_lab_user, update_lab, update_user,
};

pub fn users_labs_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/users", get(list_users).post(create_user))
        .route("/api/v1/users/{id}", patch(update_user))
        .route("/api/v1/labs", get(list_labs).post(create_lab))
        .route("/api/v1/labs/{id}", get(get_lab).patch(update_lab))
        .route(
            "/api/v1/labs/{id}/users",
            get(list_lab_users).post(assign_lab_user),
        )
        .route("/api/v1/labs/{id}/users/{user_id}", delete(remove_lab_user))
}
