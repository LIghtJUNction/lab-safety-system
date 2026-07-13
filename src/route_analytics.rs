use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use sqlx::{PgPool, Row};

use crate::{
    models::*,
    route_permissions::{is_system_admin, require_lab_access},
    route_support::{ApiError, AppState, ListQuery},
    routes::require_user,
};

pub(crate) async fn dashboard_stats(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<DashboardStats>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    let exam_counts = sqlx::query(
        r#"
        select count(*)::bigint as total,
               count(*) filter (where exam_results.status = 'passed')::bigint as passed
        from exam_results
        join trainings on trainings.id = exam_results.training_id
        where (
            $1::boolean
            or exists(
                select 1 from lab_users
                where lab_users.lab_id = trainings.lab_id and lab_users.user_id = $2
            )
        )
          and ($3::bigint is null or trainings.lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?;
    let total: i64 = exam_counts.get("total");
    let passed: i64 = exam_counts.get("passed");
    let training_count: i64 = sqlx::query(
        r#"
        select count(*)::bigint as count
        from trainings
        where (
            $1::boolean
            or exists(
                select 1 from lab_users
                where lab_users.lab_id = trainings.lab_id and lab_users.user_id = $2
            )
        )
          and ($3::bigint is null or trainings.lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?
    .get("count");
    let incident_count: i64 = sqlx::query(
        r#"
        select count(*)::bigint as count
        from incident_cases
        where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $2))
          and ($3::bigint is null or lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?
    .get("count");
    let equipment_count: i64 = sqlx::query(
        r#"
        select count(*)::bigint as count
        from equipment
        where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2))
          and ($3::bigint is null or lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?
    .get("count");
    let open_repairs: i64 = sqlx::query(
        r#"
        select count(*)::bigint as count
        from repair_tickets
        join equipment on equipment.id = repair_tickets.equipment_id
        where repair_tickets.status = 'open'
          and ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = equipment.lab_id and lab_users.user_id = $2))
          and ($3::bigint is null or equipment.lab_id = $3)
        "#,
    )
    .bind(is_system_admin(&actor))
    .bind(actor.id)
    .bind(query.lab_id)
    .fetch_one(&state.pool)
    .await?
    .get("count");
    Ok(Json(DashboardStats {
        regulation_count: table_count(&state.pool, "regulations").await?,
        incident_count,
        training_count,
        equipment_count,
        open_repair_count: open_repairs,
        exam_pass_rate: if total == 0 {
            0.0
        } else {
            passed as f64 / total as f64
        },
    }))
}

pub(crate) async fn incident_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<IncidentAnalytics>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    Ok(Json(IncidentAnalytics {
        by_category: count_incident_buckets(&state.pool, "category", &actor, query.lab_id).await?,
        by_severity: count_incident_buckets(&state.pool, "severity", &actor, query.lab_id).await?,
    }))
}

pub(crate) async fn regulation_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<RegulationAnalytics>, ApiError> {
    require_user(&state, &headers).await?;
    Ok(Json(RegulationAnalytics {
        by_type: count_buckets(
            &state.pool,
            "select regulation_type as name, count(*)::bigint as count from regulations group by regulation_type order by count desc",
        )
        .await?,
        by_authority: count_buckets(
            &state.pool,
            "select issuing_authority as name, count(*)::bigint as count from regulations group by issuing_authority order by count desc",
        )
        .await?,
    }))
}

pub(crate) async fn hazard_analytics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<ListQuery>,
) -> Result<Json<HazardAnalytics>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if let Some(lab_id) = query.lab_id {
        require_lab_access(&state.pool, &actor, lab_id).await?;
    }
    Ok(Json(HazardAnalytics {
        by_status: count_hazard_buckets(&state.pool, "status", &actor, query.lab_id).await?,
        by_category: count_hazard_buckets(&state.pool, "category", &actor, query.lab_id).await?,
    }))
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

async fn count_incident_buckets(
    pool: &PgPool,
    column: &'static str,
    actor: &AuthUser,
    lab_id: Option<i64>,
) -> Result<Vec<CountBucket>, ApiError> {
    let sql = match column {
        "category" => {
            r#"
            select category as name, count(*)::bigint as count
            from incident_cases
            where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $2))
              and ($3::bigint is null or lab_id = $3)
            group by category
            order by count desc
            "#
        }
        "severity" => {
            r#"
            select severity as name, count(*)::bigint as count
            from incident_cases
            where ($1::boolean or exists(select 1 from lab_users where lab_users.lab_id = incident_cases.lab_id and lab_users.user_id = $2))
              and ($3::bigint is null or lab_id = $3)
            group by severity
            order by count desc
            "#
        }
        _ => return Err(ApiError::bad_request("Unsupported analytics column")),
    };
    let rows = sqlx::query(sql)
        .bind(is_system_admin(actor))
        .bind(actor.id)
        .bind(lab_id)
        .fetch_all(pool)
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| CountBucket {
            name: row.get("name"),
            count: row.get("count"),
        })
        .collect())
}

async fn count_hazard_buckets(
    pool: &PgPool,
    column: &'static str,
    actor: &AuthUser,
    lab_id: Option<i64>,
) -> Result<Vec<CountBucket>, ApiError> {
    let sql = match column {
        "status" => {
            r#"
            select status as name, count(*)::bigint as count
            from safety_hazards
            where (
                $1::boolean
                or reported_by = $2
                or responsible_user_id = $2
                or exists(select 1 from lab_users where lab_users.lab_id = safety_hazards.lab_id and lab_users.user_id = $2)
              )
              and ($3::bigint is null or lab_id = $3)
            group by status
            order by count desc
            "#
        }
        "category" => {
            r#"
            select category as name, count(*)::bigint as count
            from safety_hazards
            where (
                $1::boolean
                or reported_by = $2
                or responsible_user_id = $2
                or exists(select 1 from lab_users where lab_users.lab_id = safety_hazards.lab_id and lab_users.user_id = $2)
              )
              and ($3::bigint is null or lab_id = $3)
            group by category
            order by count desc
            "#
        }
        _ => return Err(ApiError::bad_request("Unsupported analytics column")),
    };
    let rows = sqlx::query(sql)
        .bind(is_system_admin(actor))
        .bind(actor.id)
        .bind(lab_id)
        .fetch_all(pool)
        .await?;
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
