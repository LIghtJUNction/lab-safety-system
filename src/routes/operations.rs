use std::sync::Arc;

use axum::{
    Router,
    routing::{get, patch},
};

use crate::route_operations::{
    create_booking, create_equipment, create_exam_result, create_repair, create_training,
    list_bookings, list_equipment, list_exam_results, list_repairs, list_trainings, update_repair,
};
use crate::route_support::AppState;

pub fn operations_routes() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/v1/trainings",
            get(list_trainings).post(create_training),
        )
        .route(
            "/api/v1/exam-results",
            get(list_exam_results).post(create_exam_result),
        )
        .route(
            "/api/v1/equipment",
            get(list_equipment).post(create_equipment),
        )
        .route(
            "/api/v1/equipment-bookings",
            get(list_bookings).post(create_booking),
        )
        .route(
            "/api/v1/repair-tickets",
            get(list_repairs).post(create_repair),
        )
        .route("/api/v1/repair-tickets/{id}", patch(update_repair))
}
