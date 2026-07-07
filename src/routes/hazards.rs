use std::sync::Arc;

use axum::{
    Router,
    routing::{get, patch, post},
};

use crate::route_hazards::{
    claim_hazard, create_hazard, list_hazards, remediate_hazard, update_hazard_status,
};
use crate::route_uploads::{
    upload_hazard_issue_photo, upload_hazard_remediation_photo,
};
use crate::route_support::AppState;

pub fn hazards_routes() -> Router<Arc<AppState>> {
    Router::new()
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
}
