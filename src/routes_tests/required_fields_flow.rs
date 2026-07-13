use super::*;

#[tokio::test]
async fn operation_creates_require_explicit_business_fields() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let lab_id = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'Required fields lab', 'active') returning id",
    )
    .bind(format!("REQUIRED-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    sqlx::query("insert into lab_users (lab_id, user_id, lab_role) values ($1, $2, 'lab_member')")
        .bind(lab_id)
        .bind(ctx.researcher_id)
        .execute(&ctx.pool)
        .await?;

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/trainings",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "Missing passing score",
            "target_role": "lab_member",
            "status": "active"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, training) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/trainings",
        Some(&ctx.admin_token),
        serde_json::json!({
            "lab_id": lab_id,
            "title": "Backend scored training",
            "target_role": "lab_member",
            "status": "active",
            "starts_on": "2026-07-01",
            "exam_required_score": 80
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, failed_exam) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/exam-results",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "training_id": training["id"],
            "user_id": ctx.researcher_id,
            "score": 79
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(failed_exam["status"], "failed");

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/equipment",
        Some(&ctx.admin_token),
        serde_json::json!({
            "asset_code": format!("NO-STATUS-{}", ctx.schema),
            "name": "Missing status equipment",
            "lab_name": "公共设备平台"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, equipment) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/equipment",
        Some(&ctx.admin_token),
        serde_json::json!({
            "asset_code": format!("PUMP-{}", ctx.schema),
            "name": "真空泵",
            "lab_name": "公共设备平台",
            "status": "available"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/repair-tickets",
        Some(&ctx.admin_token),
        serde_json::json!({
            "equipment_id": equipment["id"],
            "reported_by": ctx.admin_id,
            "description": "缺少显式状态"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/repair-tickets",
        Some(&ctx.admin_token),
        serde_json::json!({
            "equipment_id": equipment["id"],
            "reported_by": ctx.admin_id,
            "description": "不能直接创建关闭工单",
            "status": "closed"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, repair) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/repair-tickets",
        Some(&ctx.admin_token),
        serde_json::json!({
            "equipment_id": equipment["id"],
            "reported_by": ctx.admin_id,
            "description": "泵压异常",
            "status": "open"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/repair-tickets/{}", repair["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({ "status": "nonsense" }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, managed_user) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/users",
        Some(&ctx.admin_token),
        serde_json::json!({
            "username": format!("role_check_{}", ctx.schema),
            "display_name": "Role Check",
            "email": format!("role-check-{}@example.com", ctx.schema),
            "role": "lab_member",
            "auth_provider": "password",
            "password": "RoleCheckStrong123!"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/users/{}", managed_user["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({ "role": "system_admin" }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, lab) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/labs",
        Some(&ctx.admin_token),
        serde_json::json!({
            "code": format!("ROLE-LAB-{}", ctx.schema),
            "name": "Role validation lab",
            "status": "active"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/labs/{}", lab["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({ "status": "retired" }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/labs/{}/users", lab["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({
            "user_id": managed_user["id"],
            "lab_role": "owner"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    Ok(())
}
