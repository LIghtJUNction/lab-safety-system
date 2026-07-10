use std::sync::Arc;

use axum::{
    Json,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, StatusCode},
};
use sqlx::PgPool;

use crate::{
    models::*,
    route_permissions::*,
    route_support::*,
    routes::require_user,
    security::{hash_password, validate_password_strength},
};

fn configured_federated_login(enabled: bool, login_url: Option<&str>, callback_path: &str) -> bool {
    enabled
        && login_url.is_some_and(|value| {
            let value = value.trim();
            !value.is_empty() && value != callback_path
        })
}

pub(crate) async fn resolve_lab_reference(
    pool: &PgPool,
    lab_id: Option<i64>,
    lab_name: Option<String>,
) -> Result<(Option<i64>, String), ApiError> {
    if let Some(lab_id) = lab_id {
        let name = sqlx::query_scalar::<_, String>("select name from labs where id = $1")
            .bind(lab_id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| ApiError::not_found("Lab not found"))?;
        return Ok((Some(lab_id), name));
    }

    let lab_name = lab_name
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("lab_id or lab_name is required"))?;
    Ok((None, lab_name))
}

pub(crate) async fn create_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<UserCreate>,
) -> Result<Json<User>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    if let Some(password) = payload.password.as_deref() {
        validate_password_strength(password).map_err(ApiError::bad_request)?;
    }
    let auth_provider = payload.auth_provider;
    if !matches!(auth_provider.as_str(), "password" | "sso" | "oauth") {
        return Err(ApiError::bad_request(
            "auth_provider must be password, sso, or oauth",
        ));
    }
    let auth_runtime = state.auth_runtime.read().await;
    if auth_provider == "sso"
        && !configured_federated_login(
            auth_runtime.sso_enabled,
            auth_runtime.sso_login_url.as_deref(),
            "/api/v1/auth/sso/callback",
        )
    {
        return Err(ApiError::bad_request(
            "SSO is not enabled for this deployment",
        ));
    }
    if auth_provider == "oauth"
        && !configured_federated_login(
            auth_runtime.oauth_enabled,
            auth_runtime.oauth_login_url.as_deref(),
            "/api/v1/auth/oauth/callback",
        )
    {
        return Err(ApiError::bad_request(
            "OAuth is not enabled for this deployment",
        ));
    }
    drop(auth_runtime);
    if auth_provider == "password" && payload.password.is_none() {
        return Err(ApiError::bad_request("Password users require a password"));
    }
    if auth_provider != "password" && payload.password.is_some() {
        return Err(ApiError::bad_request(
            "Federated users must not include a password",
        ));
    }
    let role = payload.role;
    validate_global_role(&role)?;
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
    .bind(role)
    .bind(auth_provider)
    .bind(payload.department)
    .bind(password_hash)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(user))
}

pub(crate) async fn list_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<User>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        if let Some(lab_id) = query.lab_id {
            require_lab_manager(&state.pool, &actor, lab_id).await?;
        } else {
            let has_admin_membership = sqlx::query_scalar::<_, bool>(
                "select exists(select 1 from lab_users where user_id = $1 and lab_role = 'lab_admin')",
            )
            .bind(actor.id)
            .fetch_one(&state.pool)
            .await?;
            if !has_admin_membership {
                return Err(ApiError::forbidden(
                    "System administrator or laboratory administrator role required",
                ));
            }
        }
    }
    let q = wildcard(query.q);
    let users = sqlx::query_as::<_, User>(
        r#"
        select distinct users.id, users.username, users.display_name, users.email, users.role, users.auth_provider, users.department, users.is_active, users.created_at
        from users
        left join lab_users visible_memberships on visible_memberships.user_id = users.id
        where ($1::text is null or users.username ilike $1 or users.display_name ilike $1 or users.email ilike $1)
          and ($2::text is null or users.role = $2)
          and (
            $3::boolean
            or exists (
              select 1
              from lab_users managed
              where managed.lab_id = visible_memberships.lab_id
                and managed.user_id = $4
                and managed.lab_role = 'lab_admin'
            )
          )
          and ($5::bigint is null or visible_memberships.lab_id = $5)
        order by users.created_at desc
        limit $6 offset $7
        "#,
    )
    .bind(q)
    .bind(query.role)
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    if !is_system_admin(&actor) && query.lab_id.is_some() && users.is_empty() {
        let lab_exists =
            sqlx::query_scalar::<_, bool>("select exists(select 1 from labs where id = $1)")
                .bind(query.lab_id)
                .fetch_one(&state.pool)
                .await?;
        if !lab_exists {
            return Err(ApiError::not_found("Lab not found"));
        }
    }
    Ok(Json(users))
}

pub(crate) async fn update_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<UserUpdate>,
) -> Result<Json<User>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;

    if let Some(role) = payload.role.as_deref() {
        validate_global_role(role)?;
    }

    let current_role = sqlx::query_scalar::<_, String>("select role from users where id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await?
        .ok_or_else(|| ApiError::not_found("User not found"))?;

    if matches!(current_role.as_str(), ROLE_SYSTEM_ADMIN | "super_admin")
        && (payload.role.is_some() || payload.is_active == Some(false))
    {
        return Err(ApiError::bad_request(
            "System administrator role and active status must be managed by CLI",
        ));
    }

    if actor.id == id && payload.is_active == Some(false) {
        return Err(ApiError::bad_request(
            "Cannot deactivate the current authenticated user",
        ));
    }

    let user = sqlx::query_as::<_, User>(
        r#"
        update users
        set display_name = coalesce($1, display_name),
            email = coalesce($2, email),
            role = coalesce($3, role),
            department = coalesce($4, department),
            is_active = coalesce($5, is_active),
            updated_at = now()
        where id = $6
        returning id, username, display_name, email, role, auth_provider, department, is_active, created_at
        "#,
    )
    .bind(payload.display_name)
    .bind(payload.email)
    .bind(payload.role)
    .bind(payload.department)
    .bind(payload.is_active)
    .bind(id)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(user))
}

pub(crate) async fn create_lab(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<LabCreate>,
) -> Result<Json<Lab>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    validate_lab_status(&payload.status)?;
    let lab = sqlx::query_as::<_, Lab>(
        r#"
        insert into labs (code, name, location, department, manager_user_id, contact, status, description)
        values ($1, $2, $3, $4, $5, $6, $7, $8)
        returning id, code, name, location, department, manager_user_id, contact, status, description, created_at
        "#,
    )
    .bind(payload.code)
    .bind(payload.name)
    .bind(payload.location)
    .bind(payload.department)
    .bind(payload.manager_user_id)
    .bind(payload.contact)
    .bind(payload.status)
    .bind(payload.description)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(lab))
}

pub(crate) async fn list_labs(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Lab>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    let labs = sqlx::query_as::<_, Lab>(
        r#"
        select id, code, name, location, department, manager_user_id, contact, status, description, created_at
        from labs
        where ($1::text is null or code ilike $1 or name ilike $1 or location ilike $1 or department ilike $1)
          and ($2::text is null or status = $2)
          and (
            $3::boolean
            or exists(select 1 from lab_users where lab_users.lab_id = labs.id and lab_users.user_id = $4)
          )
        order by name asc, id asc
        limit $5 offset $6
        "#,
    )
    .bind(wildcard(query.q))
    .bind(query.status)
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(limit(query.limit))
    .bind(offset(query.offset))
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(labs))
}

pub(crate) async fn get_lab(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Lab>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_access(&state.pool, &actor, id).await?;
    let lab = sqlx::query_as::<_, Lab>(
        r#"
        select id, code, name, location, department, manager_user_id, contact, status, description, created_at
        from labs
        where id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    lab.map(Json)
        .ok_or_else(|| ApiError::not_found("Lab not found"))
}

pub(crate) async fn update_lab(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<LabUpdate>,
) -> Result<Json<Lab>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_manager(&state.pool, &actor, id).await?;
    if let Some(status) = payload.status.as_deref() {
        validate_lab_status(status)?;
    }
    let lab = sqlx::query_as::<_, Lab>(
        r#"
        update labs
        set code = coalesce($1, code),
            name = coalesce($2, name),
            location = coalesce($3, location),
            department = coalesce($4, department),
            manager_user_id = coalesce($5, manager_user_id),
            contact = coalesce($6, contact),
            status = coalesce($7, status),
            description = coalesce($8, description),
            updated_at = now()
        where id = $9
        returning id, code, name, location, department, manager_user_id, contact, status, description, created_at
        "#,
    )
    .bind(payload.code)
    .bind(payload.name)
    .bind(payload.location)
    .bind(payload.department)
    .bind(payload.manager_user_id)
    .bind(payload.contact)
    .bind(payload.status)
    .bind(payload.description)
    .bind(id)
    .fetch_optional(&state.pool)
    .await?;
    lab.map(Json)
        .ok_or_else(|| ApiError::not_found("Lab not found"))
}

pub(crate) async fn list_lab_users(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Vec<LabUser>>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_access(&state.pool, &actor, id).await?;
    let users = sqlx::query_as::<_, LabUser>(
        r#"
        select
          lu.id, lu.lab_id, lu.user_id, lu.lab_role, lu.created_at,
          u.username, u.display_name, u.email, u.role as global_role
        from lab_users lu
        join users u on lu.user_id = u.id
        where lu.lab_id = $1
        order by
          case lu.lab_role
            when 'lab_admin' then 1
            when 'lab_member' then 2
            when 'visitor' then 3
            else 4
          end,
          lu.user_id asc
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(users))
}

pub(crate) async fn assign_lab_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<i64>,
    Json(payload): Json<LabUserAssign>,
) -> Result<Json<LabUser>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_manager(&state.pool, &actor, id).await?;
    validate_lab_role(&payload.lab_role)?;
    let assigned = sqlx::query_as::<_, LabUser>(
        r#"
        with inserted as (
          insert into lab_users (lab_id, user_id, lab_role)
          values ($1, $2, $3)
          on conflict (lab_id, user_id) do update set
            lab_role = excluded.lab_role,
            updated_at = now()
          returning id, lab_id, user_id, lab_role, created_at
        )
        select
          i.id, i.lab_id, i.user_id, i.lab_role, i.created_at,
          u.username, u.display_name, u.email, u.role as global_role
        from inserted i
        join users u on i.user_id = u.id
        "#,
    )
    .bind(id)
    .bind(payload.user_id)
    .bind(payload.lab_role)
    .fetch_one(&state.pool)
    .await?;
    Ok(Json(assigned))
}

pub(crate) async fn remove_lab_user(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    AxumPath((id, user_id)): AxumPath<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_lab_manager(&state.pool, &actor, id).await?;
    let deleted = sqlx::query(
        r#"
        delete from lab_users
        where lab_id = $1 and user_id = $2
        "#,
    )
    .bind(id)
    .bind(user_id)
    .execute(&state.pool)
    .await?;
    if deleted.rows_affected() == 0 {
        Err(ApiError::not_found("Lab user not found"))
    } else {
        Ok(StatusCode::NO_CONTENT)
    }
}
