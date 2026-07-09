use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};

use crate::route_documents::{
    create_incident, create_regulation, list_incidents, list_regulations,
};
use crate::route_support::AppState;
use crate::route_uploads::{upload_incident_file, upload_regulation_file};

pub fn documents_routes() -> Router<Arc<AppState>> {
    Router::new()
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
}
