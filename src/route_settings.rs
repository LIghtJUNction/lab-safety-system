use std::sync::Arc;

use axum::{Json, extract::State, http::HeaderMap};

use crate::{
    models::{CarouselSlide, LoginCarouselSettings, SiteSetting},
    route_permissions::is_system_admin,
    route_support::{ApiError, AppState},
    routes::require_user,
};

pub(crate) async fn get_login_carousel(
    State(state): State<Arc<AppState>>,
) -> Result<Json<LoginCarouselSettings>, ApiError> {
    let row: Option<SiteSetting> = sqlx::query_as(
        "select key, value, updated_at from site_settings where key = 'login_carousel' limit 1",
    )
    .fetch_optional(&state.pool)
    .await?;

    if let Some(row) = row {
        let parsed = serde_json::from_value::<LoginCarouselSettings>(row.value)?;
        if !parsed.zh.is_empty() || !parsed.en.is_empty() {
            return Ok(Json(parsed));
        }
    }
    Ok(Json(default_login_carousel()))
}

pub(crate) async fn update_login_carousel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(payload): Json<LoginCarouselSettings>,
) -> Result<Json<LoginCarouselSettings>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        return Err(ApiError::forbidden(
            "Only system administrator can update login carousel",
        ));
    }
    if payload.zh.is_empty() && payload.en.is_empty() {
        return Err(ApiError::bad_request(
            "At least one language carousel must have slides",
        ));
    }
    let value = serde_json::to_value(&payload)?;
    sqlx::query(
        r#"
        insert into site_settings (key, value, updated_at)
        values ('login_carousel', $1, now())
        on conflict (key) do update set value = excluded.value, updated_at = now()
        "#,
    )
    .bind(value)
    .execute(&state.pool)
    .await?;
    Ok(Json(payload))
}

pub(crate) async fn reset_login_carousel(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    if !is_system_admin(&actor) {
        return Err(ApiError::forbidden(
            "Only system administrator can reset login carousel",
        ));
    }

    sqlx::query("delete from site_settings where key = 'login_carousel'")
        .execute(&state.pool)
        .await?;

    Ok(Json(serde_json::json!({ "reset": true })))
}

fn default_login_carousel() -> LoginCarouselSettings {
    let zh = vec![
        CarouselSlide {
            stat: "隐患闭环".to_string(),
            title: "实验室安全管理平台".to_string(),
            body: "统一处理隐患上报、责任认领、整改照片、培训考核、设备预约和报修工单。"
                .to_string(),
        },
        CarouselSlide {
            stat: "分角色视图".to_string(),
            title: "管理端与普通用户分离".to_string(),
            body: "管理员聚合统计、用户和台账；普通用户只处理自己的上报、认领与整改任务。"
                .to_string(),
        },
        CarouselSlide {
            stat: "安全登录".to_string(),
            title: "支持多种身份入口".to_string(),
            body: "账号密码、Passkey、SSO 和 OAuth 可按部署环境组合使用，超级管理员仍由 CLI 控制。"
                .to_string(),
        },
    ];
    let en = vec![
        CarouselSlide {
            stat: "Hazard closure".to_string(),
            title: "Closed-loop lab safety platform".to_string(),
            body: "Track hazards, ownership, remediation photos, training, bookings, and repair tickets in one workflow.".to_string(),
        },
        CarouselSlide {
            stat: "Role-based UI".to_string(),
            title: "Separate admin and user views".to_string(),
            body: "Administrators manage analytics and registries; normal users focus on their own reports and remediation tasks.".to_string(),
        },
        CarouselSlide {
            stat: "Secure sign-in".to_string(),
            title: "Multiple identity options".to_string(),
            body: "Password, Passkey, SSO, and OAuth can be combined per deployment while super admins stay CLI-governed.".to_string(),
        },
    ];
    LoginCarouselSettings { zh, en }
}
