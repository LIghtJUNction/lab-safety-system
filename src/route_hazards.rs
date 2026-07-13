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
        ApiError, AppState, HAZARD_STATUS_OPEN, ListQuery, ROLE_LAB_ADMIN, ROLE_LAB_MEMBER, limit,
        normalize_hazard_status, offset, validate_hazard_status, validate_optional_upload_url,
        validate_upload_url, wildcard,
    },
    route_users_labs::resolve_lab_reference,
    routes::require_user,
};

async fn require_hazard_read_access(
    state: &AppState,
    actor: &AuthUser,
    hazard: &SafetyHazard,
) -> Result<(), ApiError> {
    if is_system_admin(actor)
        || actor.id == hazard.reported_by
        || Some(actor.id) == hazard.responsible_user_id
    {
        return Ok(());
    }
    if let Some(lab_id) = hazard.lab_id {
        require_lab_access(&state.pool, actor, lab_id).await
    } else {
        Err(ApiError::forbidden("Hazard access required"))
    }
}

pub(crate) async fn create_hazard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<SafetyHazardCreate>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    ensure_self_or_admin(&actor, payload.reported_by)?;
    let (lab_id, lab_name) =
        resolve_lab_reference(&state.pool, payload.lab_id, payload.lab_name).await?;
    // Multi-lab model: hazards must bind to a real lab for all roles, including system_admin.
    let Some(lab_id) = lab_id else {
        return Err(ApiError::bad_request("lab_id is required"));
    };
    require_lab_role(
        &state.pool,
        &actor,
        lab_id,
        &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
    )
    .await?;
    validate_optional_upload_url(
        payload.issue_photo_url.as_deref(),
        "/uploads/hazards/issue/",
        "issue_photo_url",
    )?;
    // New hazards always start as canonical `open` (not legacy `reported`).
    let mut transaction = state.pool.begin().await?;
    let hazard = sqlx::query_as::<_, SafetyHazard>(
        r#"
        insert into safety_hazards (title, lab_id, lab_name, category, description, reported_by, issue_photo_url, status)
        values ($1, $2, $3, $4, $5, $6, $7, $8)
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.title)
    .bind(lab_id)
    .bind(lab_name)
    .bind(payload.category)
    .bind(payload.description)
    .bind(payload.reported_by)
    .bind(payload.issue_photo_url)
    .bind(HAZARD_STATUS_OPEN)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        "insert into hazard_status_events (hazard_id, from_status, to_status, actor_user_id) values ($1, null, $2, $3)",
    )
    .bind(hazard.id)
    .bind(HAZARD_STATUS_OPEN)
    .bind(actor.id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(hazard))
}

pub(crate) async fn list_hazards(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<SafetyHazard>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let rows = sqlx::query_as::<_, SafetyHazard>(
        r#"
        select id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        from safety_hazards
        where ($1::text is null or title ilike $1 or description ilike $1)
          and ($2::text is null or status = $2)
          and ($3::bigint is null or responsible_user_id = $3)
          and ($4::bigint is null or reported_by = $4)
          and (
            $5::boolean
            or reported_by = $6
            or responsible_user_id = $6
            or exists(select 1 from lab_users where lab_users.lab_id = safety_hazards.lab_id and lab_users.user_id = $6)
          )
          and ($7::bigint is null or lab_id = $7)
        order by created_at desc
        limit $8 offset $9
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(query.responsible_user_id)
    .bind(query.reported_by)
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

pub(crate) async fn get_hazard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let hazard = sqlx::query_as::<_, SafetyHazard>(
        r#"
        select id, lab_id, title, lab_name, category, description, status, reported_by,
               responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note,
               created_at
        from safety_hazards
        where id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Hazard not found"))?;
    require_hazard_read_access(&state, &actor, &hazard).await?;
    Ok(Json(hazard))
}

pub(crate) async fn list_hazard_history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Vec<HazardStatusEvent>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let hazard = sqlx::query_as::<_, SafetyHazard>(
        r#"
        select id, lab_id, title, lab_name, category, description, status, reported_by,
               responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note,
               created_at
        from safety_hazards
        where id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Hazard not found"))?;
    require_hazard_read_access(&state, &actor, &hazard).await?;
    let events = sqlx::query_as::<_, HazardStatusEvent>(
        r#"
        select id, hazard_id, from_status, to_status, actor_user_id, created_at
        from hazard_status_events
        where hazard_id = $1
        order by created_at asc, id asc
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(events))
}

pub(crate) async fn claim_hazard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardClaim>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, _, _) = hazard_scope(&state.pool, id).await?;
    if is_system_admin(&actor) {
        // Global system administrators may assign any responsible user.
    } else if let Some(lab_id) = lab_id {
        if is_lab_admin(&state.pool, lab_id, actor.id).await? {
            require_lab_access(&state.pool, &actor, lab_id).await?;
        } else {
            ensure_self_or_admin(&actor, payload.responsible_user_id)?;
            require_lab_role(
                &state.pool,
                &actor,
                lab_id,
                &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
            )
            .await?;
        }
    } else {
        ensure_self_or_admin(&actor, payload.responsible_user_id)?;
    }
    let mut transaction = state.pool.begin().await?;
    let current_status = sqlx::query_scalar::<_, String>(
        "select status from safety_hazards where id = $1 for update",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| ApiError::not_found("Hazard not found"))?;
    if normalize_hazard_status(&current_status) != HAZARD_STATUS_OPEN {
        return Err(ApiError::conflict("Only open hazards can be claimed"));
    }
    let hazard = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards set responsible_user_id = $1, status = 'claimed', updated_at = now()
        where id = $2
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.responsible_user_id)
    .bind(id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        "insert into hazard_status_events (hazard_id, from_status, to_status, actor_user_id) values ($1, $2, 'claimed', $3)",
    )
    .bind(id)
    .bind(normalize_hazard_status(&current_status))
    .bind(actor.id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(hazard))
}

pub(crate) async fn remediate_hazard(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardRemediation>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, _, responsible_user_id) = hazard_scope(&state.pool, id).await?;
    if let Some(lab_id) = lab_id
        && !is_system_admin(&actor)
        && !is_lab_admin(&state.pool, lab_id, actor.id).await?
    {
        if responsible_user_id != Some(actor.id) {
            return Err(ApiError::forbidden(
                "Cannot remediate another user's hazard",
            ));
        }
        require_lab_role(
            &state.pool,
            &actor,
            lab_id,
            &[ROLE_LAB_ADMIN, ROLE_LAB_MEMBER],
        )
        .await?;
    }
    validate_upload_url(
        &payload.remediation_photo_url,
        "/uploads/hazards/remediation/",
        "remediation_photo_url",
    )?;
    let mut transaction = state.pool.begin().await?;
    let current_status = sqlx::query_scalar::<_, String>(
        "select status from safety_hazards where id = $1 for update",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| ApiError::not_found("Hazard not found"))?;
    if current_status != "claimed" {
        return Err(ApiError::conflict(
            "Only claimed hazards can submit remediation",
        ));
    }
    let hazard = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards
        set remediation_photo_url = $1, remediation_note = $2, status = 'remediation_submitted', updated_at = now()
        where id = $3 and responsible_user_id is not null
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(payload.remediation_photo_url)
    .bind(payload.remediation_note)
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await?;
    let hazard = hazard.ok_or_else(|| {
        ApiError::bad_request("Hazard must exist and be claimed before remediation")
    })?;
    sqlx::query(
        "insert into hazard_status_events (hazard_id, from_status, to_status, actor_user_id) values ($1, $2, 'remediation_submitted', $3)",
    )
    .bind(id)
    .bind(current_status)
    .bind(actor.id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(hazard))
}

pub(crate) async fn update_hazard_status(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<SafetyHazardStatusUpdate>,
) -> Result<Json<SafetyHazard>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let (lab_id, _, _) = hazard_scope(&state.pool, id).await?;
    if let Some(lab_id) = lab_id {
        require_lab_manager(&state.pool, &actor, lab_id).await?;
    } else {
        require_admin(&actor)?;
    }
    validate_hazard_status(&payload.status)?;
    // Persist canonical form so legacy `reported` becomes `open`.
    let status = normalize_hazard_status(&payload.status).to_owned();
    let mut transaction = state.pool.begin().await?;
    let stored_status = sqlx::query_scalar::<_, String>(
        "select status from safety_hazards where id = $1 for update",
    )
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await?
    .ok_or_else(|| ApiError::not_found("Hazard not found"))?;
    let current_status = normalize_hazard_status(&stored_status);
    let valid_transition = matches!(
        (current_status, status.as_str()),
        ("remediation_submitted", "closed") | ("closed", "remediation_submitted")
    );
    if !valid_transition {
        return Err(ApiError::conflict(format!(
            "Hazard cannot transition from {current_status} to {status}"
        )));
    }
    let hazard = sqlx::query_as::<_, SafetyHazard>(
        r#"
        update safety_hazards set status = $1, updated_at = now()
        where id = $2
        returning id, lab_id, title, lab_name, category, description, status, reported_by, responsible_user_id, issue_photo_url, remediation_photo_url, remediation_note, created_at
        "#,
    )
    .bind(&status)
    .bind(id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query(
        "insert into hazard_status_events (hazard_id, from_status, to_status, actor_user_id) values ($1, $2, $3, $4)",
    )
    .bind(id)
    .bind(current_status)
    .bind(&status)
    .bind(actor.id)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;
    Ok(Json(hazard))
}
