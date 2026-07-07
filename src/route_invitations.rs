use std::sync::Arc;

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;

use crate::{
    models::{Invitation, InvitationCreate, InvitationPublicInfo, InvitationRegister, InvitedUser},
    route_permissions::{is_system_admin, require_lab_manager},
    route_support::{ApiError, AppState, validate_lab_role},
    routes::require_user,
    security::{hash_password, validate_password_strength},
};

pub(crate) async fn create_invitation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<InvitationCreate>,
) -> Result<Json<Invitation>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_manager(&state.pool, &actor, payload.lab_id).await?;

    validate_lab_role(&payload.target_role)?;
    if let Some(max_uses) = payload.max_uses
        && max_uses <= 0
    {
        return Err(ApiError::bad_request(
            "Invitation max_uses must be positive",
        ));
    }

    let code = uuid::Uuid::new_v4().to_string();

    let invitation = sqlx::query_as::<_, Invitation>(
        r#"
        insert into invitations (code, lab_id, target_role, max_uses, memo, created_by, expires_at)
        values ($1, $2, $3, $4, $5, $6, $7)
        returning id, code, lab_id, target_role, max_uses, used_count, memo, created_by, created_at, expires_at, status
        "#,
    )
    .bind(code)
    .bind(payload.lab_id)
    .bind(payload.target_role)
    .bind(payload.max_uses)
    .bind(payload.memo)
    .bind(actor.id)
    .bind(payload.expires_at)
    .fetch_one(&state.pool)
    .await?;

    Ok(Json(invitation))
}

#[derive(Debug, Deserialize)]
pub struct ListInvitationsQuery {
    pub lab_id: Option<i64>,
}

pub(crate) async fn list_invitations(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListInvitationsQuery>,
) -> Result<Json<Vec<Invitation>>, ApiError> {
    let actor = require_user(&state, &headers).await?;

    let invitations = if is_system_admin(&actor) {
        if let Some(lab_id) = query.lab_id {
            sqlx::query_as::<_, Invitation>(
                "select * from invitations where lab_id = $1 order by created_at desc",
            )
            .bind(lab_id)
            .fetch_all(&state.pool)
            .await?
        } else {
            sqlx::query_as::<_, Invitation>("select * from invitations order by created_at desc")
                .fetch_all(&state.pool)
                .await?
        }
    } else {
        let lab_ids = sqlx::query_scalar::<_, i64>(
            "select lab_id from lab_users where user_id = $1 and lab_role = 'lab_admin'",
        )
        .bind(actor.id)
        .fetch_all(&state.pool)
        .await?;

        if lab_ids.is_empty() {
            return Ok(Json(vec![]));
        }

        if let Some(lab_id) = query.lab_id {
            if !lab_ids.contains(&lab_id) {
                return Err(ApiError::forbidden("You are not a manager of this lab"));
            }
            sqlx::query_as::<_, Invitation>(
                "select * from invitations where lab_id = $1 order by created_at desc",
            )
            .bind(lab_id)
            .fetch_all(&state.pool)
            .await?
        } else {
            sqlx::query_as::<_, Invitation>(
                "select * from invitations where lab_id = any($1) order by created_at desc",
            )
            .bind(&lab_ids)
            .fetch_all(&state.pool)
            .await?
        }
    };

    Ok(Json(invitations))
}

pub(crate) async fn delete_invitation(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    let actor = require_user(&state, &headers).await?;

    let invite = sqlx::query_as::<_, Invitation>("select * from invitations where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Invitation not found"))?;

    require_lab_manager(&state.pool, &actor, invite.lab_id).await?;

    sqlx::query("delete from invitations where id = $1")
        .bind(id)
        .execute(&state.pool)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn get_invitation_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Vec<InvitedUser>>, ApiError> {
    let actor = require_user(&state, &headers).await?;

    let invite = sqlx::query_as::<_, Invitation>("select * from invitations where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Invitation not found"))?;

    require_lab_manager(&state.pool, &actor, invite.lab_id).await?;

    let users = sqlx::query_as::<_, InvitedUser>(
        r#"
        select id, username, display_name, email, created_at
        from users
        where invitation_id = $1
        order by created_at desc
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;

    Ok(Json(users))
}

pub(crate) async fn get_public_invitation(
    State(state): State<Arc<AppState>>,
    AxumPath(code): AxumPath<String>,
) -> Result<Json<InvitationPublicInfo>, ApiError> {
    let invite = sqlx::query_as::<_, Invitation>(
        "select * from invitations where code = $1 and status = 'active'",
    )
    .bind(&code)
    .fetch_optional(&state.pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Invitation link not found or disabled"))?;

    if let Some(expires_at) = invite.expires_at
        && expires_at < chrono::Utc::now()
    {
        return Err(ApiError::bad_request("Invitation link has expired"));
    }

    if let Some(max_uses) = invite.max_uses
        && invite.used_count >= max_uses
    {
        return Err(ApiError::bad_request("Invitation link usage limit reached"));
    }

    let lab_name = sqlx::query_scalar::<_, String>("select name from labs where id = $1")
        .bind(invite.lab_id)
        .fetch_one(&state.pool)
        .await?;
    let inviter_name =
        sqlx::query_scalar::<_, String>("select display_name from users where id = $1")
            .bind(invite.created_by)
            .fetch_one(&state.pool)
            .await?;

    Ok(Json(InvitationPublicInfo {
        code: invite.code,
        lab_name,
        target_role: invite.target_role,
        inviter_name,
    }))
}

pub(crate) async fn register_by_invitation(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<InvitationRegister>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let mut tx = state.pool.begin().await?;
    let invite = sqlx::query_as::<_, Invitation>(
        "select * from invitations where code = $1 and status = 'active' for update",
    )
    .bind(&payload.code)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or_else(|| ApiError::not_found("Invitation link not found or disabled"))?;

    if let Some(expires_at) = invite.expires_at
        && expires_at < chrono::Utc::now()
    {
        return Err(ApiError::bad_request("Invitation link has expired"));
    }

    if let Some(max_uses) = invite.max_uses
        && invite.used_count >= max_uses
    {
        return Err(ApiError::bad_request("Invitation link usage limit reached"));
    }

    validate_lab_role(&invite.target_role)?;
    validate_password_strength(&payload.password).map_err(ApiError::bad_request)?;

    let password_hash = hash_password(&payload.password);

    let global_role = match invite.target_role.as_str() {
        "visitor" => "visitor",
        _ => "lab_member",
    };

    let user_id = match sqlx::query_scalar::<_, i64>(
        r#"
        insert into users (username, display_name, email, role, auth_provider, password_hash, invitation_id)
        values ($1, $2, $3, $4, 'password', $5, $6)
        returning id
        "#,
    )
    .bind(&payload.username)
    .bind(&payload.display_name)
    .bind(&payload.email)
    .bind(global_role)
    .bind(password_hash)
    .bind(invite.id)
    .fetch_one(&mut *tx)
    .await {
        Ok(id) => id,
        Err(sqlx::Error::Database(db_err)) if db_err.is_unique_violation() => {
            return Err(ApiError::bad_request("Username or email already exists"));
        }
        Err(e) => return Err(e.into()),
    };

    sqlx::query(
        r#"
        insert into lab_users (lab_id, user_id, lab_role)
        values ($1, $2, $3)
        "#,
    )
    .bind(invite.lab_id)
    .bind(user_id)
    .bind(&invite.target_role)
    .execute(&mut *tx)
    .await?;

    sqlx::query("update invitations set used_count = used_count + 1 where id = $1")
        .bind(invite.id)
        .execute(&mut *tx)
        .await?;

    tx.commit().await?;

    Ok(Json(
        serde_json::json!({ "status": "success", "username": payload.username }),
    ))
}
