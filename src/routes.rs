use std::{path::Path, sync::Arc};

use axum::{
    extract::{Multipart, Path as AxumPath, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use sqlx::{PgPool, Row};
use tokio::fs;
use uuid::Uuid;

use crate::{
    config::Settings,
    models::*,
    security::{create_access_token, hash_password, validate_password_strength, verify_password},
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub settings: Settings,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }
}

impl<E> From<E> for ApiError
where
    E: std::error::Error,
{
    fn from(error: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(serde_json::json!({ "detail": self.message })),
        )
            .into_response()
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    q: Option<String>,
    status: Option<String>,
    role: Option<String>,
    responsible_user_id: Option<i64>,
    reported_by: Option<i64>,
    limit: Option<i64>,
    offset: Option<i64>,
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/health", get(health))
        .route("/api/v1/ready", get(ready))
        .route("/api/v1/auth/methods", get(auth_methods))
        .route("/api/v1/auth/password-login", post(password_login))
        .route("/api/v1/users", get(list_users).post(create_user))
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
        .route("/api/v1/analytics/dashboard", get(dashboard_stats))
        .route("/api/v1/analytics/incidents", get(incident_analytics))
        .route("/api/v1/analytics/hazards", get(hazard_analytics))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn ready(State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, ApiError> {
    sqlx::query("select 1").execute(&state.pool).await?;
    Ok(Json(serde_json::json!({ "status": "ready" })))
}

async fn auth_methods(State(state): State<Arc<AppState>>) -> Json<AuthMethods> {
    Json(AuthMethods {
        password: true,
        sso: state.settings.sso_enabled,
        oauth: state.settings.oauth_enabled,
        sso_login_url: state.settings.sso_login_url.clone(),
        oauth_login_url: state.settings.oauth_login_url.clone(),
    })
}

async fn password_login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<PasswordLogin>,
) -> Result<Json<AuthToken>, ApiError> {
    let row = sqlx::query(
        r#"
        select id, username, display_name, email, role, auth_provider, password_hash, is_active
        from users
        where username = $1
        "#,
    )
    .bind(&payload.username)
    .fetch_optional(&state.pool)
    .await?;
    let Some(row) = row else {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid username or password".into(),
        });
    };
    let password_hash: Option<String> = row.try_get("password_hash")?;
    let active: bool = row.try_get("is_active")?;
    if !active || !verify_password(&payload.password, password_hash.as_deref()) {
        return Err(ApiError {
            status: StatusCode::UNAUTHORIZED,
            message: "Invalid username or password".into(),
        });
    }
    let token = create_access_token(
        row.try_get::<String, _>("username")?.as_str(),
        &state.settings.secret_key,
        state.settings.token_ttl_seconds,
    )
    .map_err(|error| ApiError {
        status: StatusCode::INTERNAL_SERVER_ERROR,
        message: error.to_string(),
    })?;
    Ok(Json(AuthToken {
        access_token: token,
        token_type: "bearer",
        expires_in: state.settings.token_ttl_seconds,
        user: AuthUser {
            id: row.try_get("id")?,
            username: row.try_get("username")?,
            display_name: row.try_get("display_name")?,
            email: row.try_get("email")?,
            role: row.try_get("role")?,
            auth_provider: row.try_get("auth_provider")?,
        },
    }))
}

async fn create_user(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<UserCreate>,
) -> Result<Json<User>, ApiError> {
    if let Some(password) = payload.password.as_deref() {
        validate_password_strength(password).map_err(ApiError::bad_request)?;
    }
    let password_hash = payload.password.as_deref().map(hash_password);
    let user = sqlx::query_as::<_, User>(
        r#"
        insert into users (username, display_name, email, role, auth_provider, department, password_hash)
        values ($1, $2, $3, $4, $5, $6, $7)
        returning id, username, display_name, email, role, auth_provider, department, is_active, created_at
        "#,
    )
    .bind(payload.username)
    .bind(payload.display_name)
    .bind(payload.email)
    .bind(payload.role.unwrap_or_else(|| "researcher".into()))
    .bind(payload.auth_provider.unwrap_or_else(|| "password".into()))
    .bind(payload.department)
    .bind(password_hash)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(user))
}

async fn list_users(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<User>>, ApiError> {
    let q = wildcard(query.q);
    let users = sqlx::query_as::<_, User>(
        r#"
        select id, username, display_name, email, role, auth_provider, department, is_active, created_at
        from users
        where ($1::text is null or username ilike $1 or display_name ilike $1 or email ilike $1)
          and ($2::text is null or role = $2)
        order by created_at desc
        limit $3 offset $4
        "#,
    )
    .bind(q)
    .bind(query.role)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users))
}

async fn create_regulation(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RegulationCreate>,
) -> Result<Json<Regulation>, ApiError> {
    Ok(Json(sqlx::query_as::<_, Regulation>(
        r#"
        insert into regulations (title, regulation_type, issuing_authority, effective_date, summary, file_url)
        values ($1, $2, $3, $4, $5, $6)
        returning id, title, regulation_type, issuing_authority, effective_date, summary, file_url, created_at
        "#,
    )
    .bind(payload.title)
    .bind(payload.regulation_type)
    .bind(payload.issuing_authority)
    .bind(payload.effective_date)
    .bind(payload.summary)
    .bind(payload.file_url)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_regulations(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Regulation>>, ApiError> {
    let rows = sqlx::query_as::<_, Regulation>(
        r#"
        select id, title, regulation_type, issuing_authority, effective_date, summary, file_url, created_at
        from regulations
        where ($1::text is null or title ilike $1)
        order by created_at desc
        limit $2 offset $3
        "#,
    )
    .bind(wildcard(query.q))
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_incident(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<IncidentCaseCreate>,
) -> Result<Json<IncidentCase>, ApiError> {
    Ok(Json(sqlx::query_as::<_, IncidentCase>(
        r#"
        insert into incident_cases (title, lab_name, occurred_on, severity, category, root_cause, corrective_actions)
        values ($1, $2, $3, $4, $5, $6, $7)
        returning id, title, lab_name, occurred_on, severity, category, root_cause, corrective_actions, created_at
        "#,
    )
    .bind(payload.title)
    .bind(payload.lab_name)
    .bind(payload.occurred_on)
    .bind(payload.severity)
    .bind(payload.category)
    .bind(payload.root_cause)
    .bind(payload.corrective_actions)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_incidents(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<IncidentCase>>, ApiError> {
    let rows = sqlx::query_as::<_, IncidentCase>(
        r#"
        select id, title, lab_name, occurred_on, severity, category, root_cause, corrective_actions, created_at
        from incident_cases
        where ($1::text is null or title ilike $1)
        order by occurred_on desc
        limit $2 offset $3
        "#,
    )
    .bind(wildcard(query.q))
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_training(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<TrainingCreate>,
) -> Result<Json<Training>, ApiError> {
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
        .bind(payload.status.unwrap_or_else(|| "draft".into()))
        .bind(payload.starts_on)
        .bind(payload.exam_required_score.unwrap_or(80))
        .fetch_one(&state.pool)
        .await?,
    ))
}

async fn list_trainings(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Training>>, ApiError> {
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

async fn create_exam_result(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ExamResultCreate>,
) -> Result<Json<ExamResult>, ApiError> {
    Ok(Json(sqlx::query_as::<_, ExamResult>(
        "insert into exam_results (training_id, user_id, score, status) values ($1, $2, $3, $4) returning id, training_id, user_id, score, status, created_at",
    )
    .bind(payload.training_id)
    .bind(payload.user_id)
    .bind(payload.score)
    .bind(payload.status)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_exam_results(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ExamResult>>, ApiError> {
    Ok(Json(sqlx::query_as::<_, ExamResult>(
        "select id, training_id, user_id, score, status, created_at from exam_results order by created_at desc",
    )
    .fetch_all(&state.pool)
    .await?))
}

async fn create_equipment(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EquipmentCreate>,
) -> Result<Json<Equipment>, ApiError> {
    Ok(Json(
        sqlx::query_as::<_, Equipment>(
            r#"
        insert into equipment (asset_code, name, lab_name, status, owner)
        values ($1, $2, $3, $4, $5)
        returning id, asset_code, name, lab_name, status, owner, created_at
        "#,
        )
        .bind(payload.asset_code)
        .bind(payload.name)
        .bind(payload.lab_name)
        .bind(payload.status.unwrap_or_else(|| "available".into()))
        .bind(payload.owner)
        .fetch_one(&state.pool)
        .await?,
    ))
}

async fn list_equipment(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Equipment>>, ApiError> {
    let rows = sqlx::query_as::<_, Equipment>(
        r#"
        select id, asset_code, name, lab_name, status, owner, created_at
        from equipment
        where ($1::text is null or name ilike $1 or asset_code ilike $1)
          and ($2::text is null or status = $2)
        order by created_at desc
        limit $3 offset $4
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn create_booking(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<EquipmentBookingCreate>,
) -> Result<Json<EquipmentBooking>, ApiError> {
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

async fn list_bookings(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<EquipmentBooking>>, ApiError> {
    Ok(Json(sqlx::query_as::<_, EquipmentBooking>(
        "select id, equipment_id, user_id, starts_at, ends_at, purpose, created_at from equipment_bookings order by starts_at desc",
    )
    .fetch_all(&state.pool)
    .await?))
}

async fn create_repair(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<RepairTicketCreate>,
) -> Result<Json<RepairTicket>, ApiError> {
    Ok(Json(sqlx::query_as::<_, RepairTicket>(
        "insert into repair_tickets (equipment_id, reported_by, description, status) values ($1, $2, $3, $4) returning id, equipment_id, reported_by, description, status, created_at",
    )
    .bind(payload.equipment_id)
    .bind(payload.reported_by)
    .bind(payload.description)
    .bind(payload.status.unwrap_or_else(|| "open".into()))
    .fetch_one(&state.pool)
    .await?))
}

async fn list_repairs(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<RepairTicket>>, ApiError> {
    Ok(Json(sqlx::query_as::<_, RepairTicket>(
        "select id, equipment_id, reported_by, description, status, created_at from repair_tickets order by created_at desc",
    )
    .fetch_all(&state.pool)
    .await?))
}

async fn update_repair(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<RepairTicketUpdate>,
) -> Result<Json<RepairTicket>, ApiError> {
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

async fn create_hazard(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<SafetyHazardCreate>,
) -> Result<Json<SafetyHazard>, ApiError> {
    Ok(Json(sqlx::query_as::<_, SafetyHazard>(
        r#"
        insert into safety_hazards (title, lab_name, category, description, reported_by, issue_photo_url)
        values ($1, $2, $3, $4, $5, $6)
        returning id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.title)
    .bind(payload.lab_name)
    .bind(payload.category)
    .bind(payload.description)
    .bind(payload.reported_by)
    .bind(payload.issue_photo_url)
    .fetch_one(&state.pool)
    .await?))
}

async fn list_hazards(
    State(state): State<Arc<AppState>>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<SafetyHazard>>, ApiError> {
    let rows = sqlx::query_as::<_, SafetyHazard>(
        r#"
        select id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        from safety_hazards
        where ($1::text is null or title ilike $1 or description ilike $1)
          and ($2::text is null or status = $2)
          and ($3::bigint is null or responsible_user_id = $3)
          and ($4::bigint is null or reported_by = $4)
        order by created_at desc
        limit $5 offset $6
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(query.responsible_user_id)
    .bind(query.reported_by)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

async fn claim_hazard(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardClaim>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let row = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards set responsible_user_id = $1, status = 'claimed', updated_at = now()
        where id = $2
        returning id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.responsible_user_id)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Hazard not found"))
}

async fn remediate_hazard(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardRemediation>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let row = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards
        set remediation_photo_url = $1, remediation_note = $2, status = 'remediation_submitted', updated_at = now()
        where id = $3 and responsible_user_id is not null
        returning id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.remediation_photo_url)
    .bind(payload.remediation_note)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::bad_request("Hazard must exist and be claimed before remediation"))
}

async fn update_hazard_status(
    State(state): State<Arc<AppState>>,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardStatusUpdate>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let row = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards set status = $1, updated_at = now()
        where id = $2
        returning id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.status)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    row.map(Json)
        .ok_or_else(|| ApiError::not_found("Hazard not found"))
}

async fn dashboard_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DashboardStats>, ApiError> {
    let total: i64 = sqlx::query("select count(*)::bigint as count from exam_results")
        .fetch_one(&state.pool)
        .await?
        .get("count");
    let passed: i64 =
        sqlx::query("select count(*)::bigint as count from exam_results where status = 'passed'")
            .fetch_one(&state.pool)
            .await?
            .get("count");
    let open_repairs: i64 =
        sqlx::query("select count(*)::bigint as count from repair_tickets where status = 'open'")
            .fetch_one(&state.pool)
            .await?
            .get("count");
    Ok(Json(DashboardStats {
        regulation_count: table_count(&state.pool, "regulations").await?,
        incident_count: table_count(&state.pool, "incident_cases").await?,
        training_count: table_count(&state.pool, "trainings").await?,
        equipment_count: table_count(&state.pool, "equipment").await?,
        open_repair_count: open_repairs,
        exam_pass_rate: if total == 0 {
            0.0
        } else {
            passed as f64 / total as f64
        },
    }))
}

async fn incident_analytics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<IncidentAnalytics>, ApiError> {
    Ok(Json(IncidentAnalytics {
        by_category: count_buckets(&state.pool, "select category as name, count(*)::bigint as count from incident_cases group by category order by count desc").await?,
        by_severity: count_buckets(&state.pool, "select severity as name, count(*)::bigint as count from incident_cases group by severity order by count desc").await?,
    }))
}

async fn hazard_analytics(
    State(state): State<Arc<AppState>>,
) -> Result<Json<HazardAnalytics>, ApiError> {
    Ok(Json(HazardAnalytics {
        by_status: count_buckets(&state.pool, "select status as name, count(*)::bigint as count from safety_hazards group by status order by count desc").await?,
        by_category: count_buckets(&state.pool, "select category as name, count(*)::bigint as count from safety_hazards group by category order by count desc").await?,
    }))
}

async fn upload_regulation_file(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    save_upload(&state, multipart, "regulations")
        .await
        .map(Json)
}

async fn upload_incident_file(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    save_upload(&state, multipart, "incidents").await.map(Json)
}

async fn upload_hazard_issue_photo(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    save_upload(&state, multipart, "hazards/issue")
        .await
        .map(Json)
}

async fn upload_hazard_remediation_photo(
    State(state): State<Arc<AppState>>,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    save_upload(&state, multipart, "hazards/remediation")
        .await
        .map(Json)
}

async fn save_upload(
    state: &AppState,
    mut multipart: Multipart,
    category: &str,
) -> Result<UploadedFile, ApiError> {
    let Some(field) = multipart.next_field().await? else {
        return Err(ApiError::bad_request("file is required"));
    };
    let filename = field.file_name().unwrap_or("upload.bin").to_string();
    let content_type = field.content_type().map(ToString::to_string);
    let bytes = field.bytes().await?;
    let safe_name = Path::new(&filename)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("upload.bin");
    let stored_name = format!("{}-{safe_name}", Uuid::new_v4());
    let target_dir = state.settings.upload_dir.join(category);
    fs::create_dir_all(&target_dir).await?;
    fs::write(target_dir.join(&stored_name), &bytes).await?;
    Ok(UploadedFile {
        filename,
        content_type,
        size: bytes.len(),
        url: format!("/uploads/{category}/{stored_name}"),
    })
}

async fn count_buckets(pool: &PgPool, sql: &str) -> Result<Vec<CountBucket>, ApiError> {
    let rows = sqlx::query(sql).fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| CountBucket {
            name: row.get("name"),
            count: row.get("count"),
        })
        .collect())
}

async fn table_count(pool: &PgPool, table: &'static str) -> Result<i64, ApiError> {
    let sql = format!("select count(*)::bigint as count from {table}");
    Ok(sqlx::query(&sql).fetch_one(pool).await?.get("count"))
}

fn wildcard(q: Option<String>) -> Option<String> {
    q.filter(|value| !value.trim().is_empty())
        .map(|value| format!("%{}%", value.trim()))
}

fn limit(value: Option<i64>) -> i64 {
    value.unwrap_or(50).clamp(1, 100)
}

fn offset(value: Option<i64>) -> i64 {
    value.unwrap_or(0).max(0)
}
