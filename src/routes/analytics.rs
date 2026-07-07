use std::sync::Arc;

use axum::{Router, routing::get};

use crate::route_analytics::{
    dashboard_stats, hazard_analytics, incident_analytics, regulation_analytics,
};
use crate::route_support::AppState;

pub fn analytics_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/v1/analytics/dashboard", get(dashboard_stats))
        .route("/api/v1/analytics/regulations", get(regulation_analytics))
        .route("/api/v1/analytics/incidents", get(incident_analytics))
        .route("/api/v1/analytics/hazards", get(hazard_analytics))
}
