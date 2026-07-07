use std::sync::Arc;

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::HeaderMap,
};

use crate::{
    models::*,
    route_permissions::*,
    route_support::{
        ApiError, AppState, ListQuery, ROLE_LAB_ADMIN, ROLE_LAB_MEMBER, limit, offset,
        validate_repair_create_status, validate_repair_status, wildcard,
    },
    route_users_labs::resolve_lab_reference,
    routes::require_user,
};

fn ensure_allowed(value: &str, allowed: &[&str], field: &str) -> Result<(), ApiError> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(ApiError::bad_request(format!("invalid {field}")))
}

fn ensure_score_range(score: i32, field: &str) -> Result<(), ApiError> {
    if (0..=100).contains(&score) {
        return Ok(());
    }
    Err(ApiError::bad_request(format!(
        "{field} must be between 0 and 100"
    )))
}

pub(crate) async fn create_training(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<TrainingCreate>,
) -> Result<Json<Training>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    ensure_allowed(
        &payload.target_role,
        &["lab_admin", "lab_member", "visitor"],
        "target_role",
    )?;
    ensure_allowed(&payload.status, &["draft", "active", "archived"], "status")?;
    ensure_score_range(payload.exam_required_score, "exam_required_score")?;
    Ok(Json(
        sqlx::query_as::<_, Training>(
            r#"
        insert into trainings (title, target_role, status, starts_on, exam_required_score)
        values ($1, $2, $3, $4, $5)
        returning id, title, target_role, status, starts_on, exam_required_score, created_at
        "#,
        )
        .bind(payload.title)
        .bind(payload.target_role)
        .bind(payload.status)
        .bind(payload.starts_on)
        .bind(payload.exam_required_score)
        .fetch_one(&state.pool)
        .await?,
    ))
}

pub(crate) async fn list_trainings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Training>>, ApiError> {
    require_user(&state, &headers).await?;
    let rows = sqlx::query_as::<_, Training>(
        "select id, title, target_role, status, starts_on, exam_required_score, created_at from trainings where ($1::text is null or status = $1) order by created_at desc limit $2 offset $3",
    )
    .bind(query.status)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub(crate) async fn create_exam_result(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<ExamResultCreate>,
) -> Result<Json<ExamResult>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.user_id)?;
    ensure_score_range(payload.score, "score")?;
    let required_score =
        sqlx::query_scalar::<_, i32>("select exam_required_score from trainings where id = $1")
            .bind(payload.training_id)
            .fetch_optional(&state.pool)
            .await?
            .ok_or_else(|| ApiError::bad_request("training not found"))?;
    let status = if payload.score >= required_score {
        "passed"
    } else {
        "failed"
    };
    Ok(Json(sqlx::query_as::<_, ExamResult>(
        "insert into exam_results (training_id, user_id, score, status) values ($1, $2, $3, $4) returning id, training_id, user_id, score, status, created_at",
    )
    .bind(payload.training_id)
    .bind(payload.user_id)
    .bind(payload.score)
    .bind(status)
    .fetch_one(&state.pool)
    .await?))
}

pub(crate) async fn list_exam_results(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Vec<ExamResult>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let rows = if is_admin(&actor) {
        sqlx::query_as::<_, ExamResult>(
            "select id, training_id, user_id, score, status, created_at from exam_results order by created_at desc",
        )
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, ExamResult>(
            "select id, training_id, user_id, score, status, created_at from exam_results where user_id = $1 order by created_at desc",
        )
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

pub(crate) async fn create_equipment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<EquipmentCreate>,
) -> Result<Json<Equipment>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, lab_name) =
        resolve_lab_reference(&state.pool, payload.lab_id, payload.lab_name).await?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    ensure_allowed(
        &payload.status,
        &["available", "in_use", "maintenance", "retired"],
        "status",
    )?;
    Ok(Json(
        sqlx::query_as::<_, Equipment>(
            r#"
        insert into equipment (asset_code, name, lab_id, lab_name, status, owner)
        values ($1, $2, $3, $4, $5, $6)
        returning id, lab_id, asset_code, name, lab_name, status, owner, created_at
        "#,
        )
        .bind(payload.asset_code)
        .bind(payload.name)
        .bind(lab_id)
        .bind(lab_name)
        .bind(payload.status)
        .bind(payload.owner)
        .fetch_one(&state.pool)
        .await?,
    ))
}

pub(crate) async fn list_equipment(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Equipment>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = sqlx::query_as::<_, Equipment>(
        r#"
        select id, lab_id, asset_code, name, lab_name, status, owner, created_at
        from equipment
        where ($1::text is null or name ilike $1 or asset_code ilike $1)
          and ($2::text is null or status = $2)
          and (
            $3::boolean
            or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $4)
          )
          and ($5::bigint is null or lab_id = $5)
        order by created_at desc
        limit $6 offset $7
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub(crate) async fn create_booking(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<EquipmentBookingCreate>,
) -> Result<Json<EquipmentBooking>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.user_id)?;
    if let Some(lab_id) = equipment_lab_id(&state.pool, payload.equipment_id).await? {
        require_lab_role(
            &state.pool,
            &actor,
            lab_id,
            &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
        )
        .await?;
    } else if !is_system_admin(&actor) {
        return Err(ApiError::forbidden("Equipment lab access required"));
    }
    if payload.ends_at <= payload.starts_at {
        return Err(ApiError::bad_request(
            "Booking end time must be later than start time",
        ));
    }
    let conflict: Option<(i64,)> = sqlx::query_as(
        "select id from equipment_bookings where equipment_id = $1 and starts_at < $2 and ends_at > $3 limit 1",
    )
    .bind(payload.equipment_id)
    .bind(payload.ends_at)
    .bind(payload.starts_at)
    .fetch_optional(&state.pool)
    .await?;
    if conflict.is_some() {
        return Err(ApiError::conflict(
            "Equipment is already booked for the selected time range",
        ));
    }
    Ok(Json(sqlx::query_as::<_, EquipmentBooking>(
        "insert into equipment_bookings (equipment_id, user_id, starts_at, ends_at, purpose) values ($1, $2, $3, $4, $5) returning id, equipment_id, user_id, starts_at, ends_at, purpose, created_at",
    )
    .bind(payload.equipment_id)
    .bind(payload.user_id)
    .bind(payload.starts_at)
    .bind(payload.ends_at)
    .bind(payload.purpose)
    .fetch_one(&state.pool)
    .await?))
}

pub(crate) async fn list_bookings(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<EquipmentBooking>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = if is_admin(&actor) {
        sqlx::query_as::<_, EquipmentBooking>(
            r#"
            select equipment_bookings.id, equipment_id, user_id, starts_at, ends_at, purpose, equipment_bookings.created_at
            from equipment_bookings
            join equipment on equipment.id = equipment_bookings.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
            order by starts_at desc
            "#,
        )
        .bind(query.lab_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, EquipmentBooking>(
            r#"
            select equipment_bookings.id, equipment_id, user_id, starts_at, ends_at, purpose, equipment_bookings.created_at
            from equipment_bookings
            join equipment on equipment.id = equipment_bookings.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
              and (
                user_id = $2
                or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2)
              )
            order by starts_at desc
            "#,
        )
        .bind(query.lab_id)
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

pub(crate) async fn create_repair(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RepairTicketCreate>,
) -> Result<Json<RepairTicket>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.reported_by)?;
    if let Some(lab_id) = equipment_lab_id(&state.pool, payload.equipment_id).await? {
        require_lab_role(
            &state.pool,
            &actor,
            lab_id,
            &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
        )
        .await?;
    } else if !is_system_admin(&actor) {
        return Err(ApiError::forbidden("Equipment lab access required"));
    }
    validate_repair_create_status(&payload.status)?;
    Ok(Json(sqlx::query_as::<_, RepairTicket>(
        "insert into repair_tickets (equipment_id, reported_by, description, status) values ($1, $2, $3, $4) returning id, equipment_id, reported_by, description, status, created_at",
    )
    .bind(payload.equipment_id)
    .bind(payload.reported_by)
    .bind(payload.description)
    .bind(payload.status)
    .fetch_one(&state.pool)
    .await?))
}

pub(crate) async fn list_repairs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<RepairTicket>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = if is_admin(&actor) {
        sqlx::query_as::<_, RepairTicket>(
            r#"
            select repair_tickets.id, repair_tickets.equipment_id, reported_by, description, repair_tickets.status, repair_tickets.created_at
            from repair_tickets
            join equipment on equipment.id = repair_tickets.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
            order by repair_tickets.created_at desc
            "#,
        )
        .bind(query.lab_id)
        .fetch_all(&state.pool)
        .await?
    } else {
        sqlx::query_as::<_, RepairTicket>(
            r#"
            select repair_tickets.id, repair_tickets.equipment_id, reported_by, description, repair_tickets.status, repair_tickets.created_at
            from repair_tickets
            join equipment on equipment.id = repair_tickets.equipment_id
            where ($1::bigint is null or equipment.lab_id = $1)
              and (
                reported_by = $2
                or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2)
              )
            order by repair_tickets.created_at desc
            "#,
        )
        .bind(query.lab_id)
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?
    };
    Ok(Json(rows))
}

pub(crate) async fn update_repair(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<RepairTicketUpdate>,
) -> Result<Json<RepairTicket>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let lab_id = sqlx::query_scalar::<_, Option<i64>>(
        r#"
        select equipment.lab_id
        from repair_tickets
        join equipment on equipment.id = repair_tickets.equipment_id
        where repair_tickets.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Repair ticket not found"))?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    validate_repair_status(&payload.status)?;
    let row = sqlx::query_as::<_, RepairTicket>(
        "update repair_tickets set status = $1, updated_at = now() where id = $2 returning id, equipment_id, reported_by, description, status, created_at",
    )
    .bind(payload.status)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Repair ticket not found"))
}
