use std::sync::Arc;

use axum::{
    Router,
    routing::{delete, get, post},
};

use crate::route_invitations::{
    create_invitation, delete_invitation, get_invitation_users, get_public_invitation,
    list_invitations, register_by_invitation,
};
use crate::route_support::AppState;

pub fn invitations_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/invitations",
            get(list_invitations).post(create_invitation),
        )
        .route("/api/v1/invitations/{id}", delete(delete_invitation))
        .route("/api/v1/invitations/{id}/users", get(get_invitation_users))
        .route(
            "/api/v1/invitations/public/{code}",
            get(get_public_invitation),
        )
        .route("/api/v1/invitations/register", post(register_by_invitation))
}
