use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
};

use crate::{
    models::{IncidentCase, IncidentCaseCreate, Regulation, RegulationCreate},
    route_permissions::{is_system_admin, require_admin, require_lab_access, require_lab_manager},
    route_support::{
        ApiError, AppState, ListQuery, limit, offset, validate_optional_upload_url, wildcard,
    },
    route_users_labs::resolve_lab_reference,
    routes::require_user,
};

pub(crate) async fn create_regulation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<RegulationCreate>,
) -> Result<Json<Regulation>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    validate_optional_upload_url(
        payload.file_url.as_deref(),
        "/uploads/regulations/",
        "file_url",
    )?;
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

pub(crate) async fn list_regulations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Regulation>>, ApiError> {
    require_user(&state, &headers).await?;
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

pub(crate) async fn get_regulation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(regulation_id): Path<i64>,
) -> Result<Json<Regulation>, ApiError> {
    require_user(&state, &headers).await?;
    let regulation = sqlx::query_as::<_, Regulation>(
        r#"
        select id, title, regulation_type, issuing_authority, effective_date, summary, file_url, created_at
        from regulations
        where id = $1
        "#,
    )
    .bind(regulation_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Regulation not found"))?;
    Ok(Json(regulation))
}

pub(crate) async fn create_incident(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<IncidentCaseCreate>,
) -> Result<Json<IncidentCase>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, lab_name) =
        resolve_lab_reference(&state.pool, payload.lab_id, payload.lab_name).await?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    validate_optional_upload_url(
        payload.file_url.as_deref(),
        "/uploads/incidents/",
        "file_url",
    )?;
    Ok(Json(sqlx::query_as::<_, IncidentCase>(
        r#"
        insert into incident_cases (title, lab_id, lab_name, occurred_on, severity, category, root_cause, corrective_actions, file_url)
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        returning id, lab_id, title, lab_name, occurred_on, severity, category, root_cause, corrective_actions, file_url, created_at
        "#,
    )
    .bind(payload.title)
    .bind(lab_id)
    .bind(lab_name)
    .bind(payload.occurred_on)
    .bind(payload.severity)
    .bind(payload.category)
    .bind(payload.root_cause)
    .bind(payload.corrective_actions)
    .bind(payload.file_url)
    .fetch_one(&state.pool)
    .await?))
}

pub(crate) async fn list_incidents(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<IncidentCase>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = sqlx::query_as::<_, IncidentCase>(
        r#"
        select id, lab_id, title, lab_name, occurred_on, severity, category, root_cause, corrective_actions, file_url, created_at
        from incident_cases
        where ($1::text is null or title ilike $1)
          and (
            $2::boolean
            or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $3)
          )
          and ($4::bigint is null or lab_id = $4)
        order by occurred_on desc
        limit $5 offset $6
        "#,
    )
    .bind(wildcard(query.q))
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub(crate) async fn get_incident(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(incident_id): Path<i64>,
) -> Result<Json<IncidentCase>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let incident = sqlx::query_as::<_, IncidentCase>(
        r#"
        select id, lab_id, title, lab_name, occurred_on, severity, category, root_cause, corrective_actions, file_url, created_at
        from incident_cases
        where id = $1
        "#,
    )
    .bind(incident_id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Incident not found"))?;
    if let Some(lab_id) = incident.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    Ok(Json(incident))
}
