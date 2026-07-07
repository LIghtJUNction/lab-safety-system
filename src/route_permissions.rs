use sqlx::{PgPool, Row};

use crate::{
    models::AuthUser,
    route_support::{ApiError, ROLE_SYSTEM_ADMIN},
};

pub(crate) fn is_admin(user: &AuthUser) -> bool {
    is_system_admin(user)
}

pub(crate) fn is_system_admin(user: &AuthUser) -> bool {
    matches!(user.role.as_str(), ROLE_SYSTEM_ADMIN | "super_admin")
}

pub(crate) fn require_admin(user: &AuthUser) -> Result<(), ApiError> {
    if is_system_admin(user) {
        Ok(())
    } else {
        Err(ApiError::forbidden("System administrator role required"))
    }
}

pub(crate) async fn is_lab_admin(
    pool: &PgPool,
    lab_id: i64,
    user_id: i64,
) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar::<_, bool>(
        "select exists(select 1 from lab_users where lab_id = $1 and user_id = $2 and lab_role = 'lab_admin')",
    )
    .bind(lab_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?)
}

pub(crate) async fn require_lab_manager(
    pool: &PgPool,
    actor: &AuthUser,
    lab_id: i64,
) -> Result<(), ApiError> {
    if is_system_admin(actor) || is_lab_admin(pool, lab_id, actor.id).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden(
            "System administrator or lab administrator role required",
        ))
    }
}

pub(crate) async fn require_lab_access(
    pool: &PgPool,
    actor: &AuthUser,
    lab_id: i64,
) -> Result<(), ApiError> {
    if is_system_admin(actor) {
        return Ok(());
    }
    let exists = sqlx::query_scalar::<_, bool>(
        "select exists(select 1 from lab_users where lab_id = $1 and user_id = $2)",
    )
    .bind(lab_id)
    .bind(actor.id)
    .fetch_one(pool)
    .await?;
    if exists {
        Ok(())
    } else {
        Err(ApiError::forbidden("Lab access required"))
    }
}

pub(crate) async fn lab_role_for_user(
    pool: &PgPool,
    lab_id: i64,
    user_id: i64,
) -> Result<Option<String>, ApiError> {
    Ok(sqlx::query_scalar::<_, String>(
        "select lab_role from lab_users where lab_id = $1 and user_id = $2",
    )
    .bind(lab_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

pub(crate) async fn require_lab_role(
    pool: &PgPool,
    actor: &AuthUser,
    lab_id: i64,
    allowed_roles: &[&str],
) -> Result<(), ApiError> {
    if is_system_admin(actor) {
        return Ok(());
    }
    let role = lab_role_for_user(pool, lab_id, actor.id).await?;
    if role
        .as_deref()
        .is_some_and(|role| allowed_roles.contains(&role))
    {
        Ok(())
    } else {
        Err(ApiError::forbidden("Insufficient lab role"))
    }
}

pub(crate) async fn hazard_scope(
    pool: &PgPool,
    hazard_id: i64,
) -> Result<(Option<i64>, i64, Option<i64>), ApiError> {
    let row = sqlx::query(
        "select lab_id, reported_by, responsible_user_id from safety_hazards where id = $1",
    )
    .bind(hazard_id)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ApiError::not_found("Hazard not found"))?;
    Ok((
        row.try_get("lab_id")?,
        row.try_get("reported_by")?,
        row.try_get("responsible_user_id")?,
    ))
}

pub(crate) async fn equipment_lab_id(
    pool: &PgPool,
    equipment_id: i64,
) -> Result<Option<i64>, ApiError> {
    let lab_id = sqlx::query_scalar::<_, Option<i64>>("select lab_id from equipment where id = $1")
        .bind(equipment_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| ApiError::not_found("Equipment not found"))?;
    Ok(lab_id)
}

pub(crate) fn ensure_self_or_admin(user: &AuthUser, target_user_id: i64) -> Result<(), ApiError> {
    if is_system_admin(user) || user.id == target_user_id {
        Ok(())
    } else {
        Err(ApiError::forbidden("Cannot manage another user's record"))
    }
}
