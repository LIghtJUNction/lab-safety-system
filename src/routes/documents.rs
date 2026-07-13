use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};

use crate::route_documents::{
    create_incident, create_regulation, get_incident, get_regulation, list_incidents,
    list_regulations,
};
use crate::route_support::AppState;
use crate::route_uploads::{upload_incident_file, upload_regulation_file};

const DOCUMENT_MULTIPART_MAX_BYTES: usize = 21 * 1024 * 1024;

pub fn documents_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/regulations",
            get(list_regulations).post(create_regulation),
        )
        .route(
            "/api/v1/regulations/upload",
            post(upload_regulation_file).layer(DefaultBodyLimit::max(DOCUMENT_MULTIPART_MAX_BYTES)),
        )
        .route("/api/v1/regulations/{id}", get(get_regulation))
        .route(
            "/api/v1/incidents",
            get(list_incidents).post(create_incident),
        )
        .route("/api/v1/incidents/{id}", get(get_incident))
        .route(
            "/api/v1/incidents/upload",
            post(upload_incident_file).layer(DefaultBodyLimit::max(DOCUMENT_MULTIPART_MAX_BYTES)),
        )
}
