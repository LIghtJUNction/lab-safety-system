use super::*;

async fn create_user_token(
    ctx: &TestApp,
    username: &str,
    role: &str,
) -> anyhow::Result<(i64, String)> {
    let user_id = sqlx::query_scalar::<_, i64>(
        r#"
        insert into users (username, display_name, email, role, auth_provider)
        values ($1, $1, $2, $3, 'password')
        returning id
        "#,
    )
    .bind(username)
    .bind(format!("{username}@example.com"))
    .bind(role)
    .fetch_one(&ctx.pool)
    .await?;
    let token = crate::security::create_access_token(
        username,
        &ctx.settings.secret_key,
        ctx.settings.token_ttl_seconds,
    )?;
    Ok((user_id, token))
}

#[tokio::test]
async fn regulation_detail_should_be_available_to_any_authenticated_user() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, regulation) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/regulations",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "Detail access regulation",
            "regulation_type": "internal",
            "issuing_authority": "Safety office",
            "effective_date": "2026-07-13",
            "summary": "Authenticated users may read this regulation.",
            "file_url": null
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "create response: {regulation}");

    let (status, detail) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/regulations/{}", regulation["id"]),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;

    assert_eq!(status, StatusCode::OK, "detail response: {detail}");
    assert_eq!(detail["id"], regulation["id"]);
    Ok(())
}

#[tokio::test]
async fn incident_detail_should_enforce_record_lab_access_and_not_found() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let (_, outsider_token) = create_user_token(&ctx, "incident_outsider", "lab_member").await?;

    let (status, lab) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/labs",
        Some(&ctx.admin_token),
        serde_json::json!({
            "code": format!("DETAIL-{}", ctx.schema),
            "name": "Incident detail lab",
            "location": "A-101",
            "department": "Safety",
            "manager_user_id": ctx.admin_id,
            "contact": "detail@example.com",
            "status": "active",
            "description": "Incident access fixture"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "lab response: {lab}");

    let (status, membership) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/labs/{}/users", lab["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({
            "user_id": ctx.researcher_id,
            "lab_role": "lab_member"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "membership response: {membership}");

    let (status, incident) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/incidents",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "Incident detail access",
            "lab_id": lab["id"],
            "occurred_on": "2026-07-13",
            "severity": "minor",
            "category": "chemical",
            "root_cause": "Fixture",
            "corrective_actions": "Verify access",
            "file_url": null
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "incident response: {incident}");

    let (member_status, member_detail) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/incidents/{}", incident["id"]),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    let (outsider_status, outsider_detail) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/incidents/{}", incident["id"]),
        Some(&outsider_token),
        Body::empty(),
        None,
    )
    .await?;
    let (missing_status, missing_detail) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/incidents/9223372036854775807",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;

    assert_eq!(
        member_status,
        StatusCode::OK,
        "member detail response: {member_detail}"
    );
    assert_eq!(member_detail["id"], incident["id"]);
    assert_eq!(
        outsider_status,
        StatusCode::FORBIDDEN,
        "outsider detail response: {outsider_detail}"
    );
    assert_eq!(
        missing_status,
        StatusCode::NOT_FOUND,
        "missing detail response: {missing_detail}"
    );
    Ok(())
}

#[tokio::test]
async fn hazard_detail_should_allow_only_privileged_or_related_users() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let (reporter_id, reporter_token) =
        create_user_token(&ctx, "hazard_reporter", "lab_member").await?;
    let (responsible_id, responsible_token) =
        create_user_token(&ctx, "hazard_responsible", "lab_member").await?;
    let (member_id, member_token) = create_user_token(&ctx, "hazard_member", "lab_member").await?;
    let (_, outsider_token) = create_user_token(&ctx, "hazard_outsider", "lab_member").await?;
    let lab_id = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'Hazard detail lab', 'active') returning id",
    )
    .bind(format!("HZ-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    sqlx::query("insert into lab_users (lab_id, user_id, lab_role) values ($1, $2, 'lab_member')")
        .bind(lab_id)
        .bind(member_id)
        .execute(&ctx.pool)
        .await?;
    let hazard_id = sqlx::query_scalar::<_, i64>(
        r#"
        insert into safety_hazards (
            title, lab_id, lab_name, category, description, status,
            reported_by, responsible_user_id
        )
        values ('Hazard detail access', $1, 'Hazard detail lab', 'chemical',
                'Permission fixture', 'claimed', $2, $3)
        returning id
        "#,
    )
    .bind(lab_id)
    .bind(reporter_id)
    .bind(responsible_id)
    .fetch_one(&ctx.pool)
    .await?;

    for (label, token) in [
        ("system admin", ctx.admin_token.as_str()),
        ("reporter", reporter_token.as_str()),
        ("responsible user", responsible_token.as_str()),
        ("lab member", member_token.as_str()),
    ] {
        let (status, detail) = request(
            &ctx.app,
            Method::GET,
            &format!("/api/v1/hazards/{hazard_id}"),
            Some(token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{label} detail response: {detail}");
        assert_eq!(detail["id"], hazard_id);
    }

    let (outsider_status, outsider_detail) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/hazards/{hazard_id}"),
        Some(&outsider_token),
        Body::empty(),
        None,
    )
    .await?;
    let (missing_status, missing_detail) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/hazards/9223372036854775807",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        outsider_status,
        StatusCode::FORBIDDEN,
        "outsider detail response: {outsider_detail}"
    );
    assert_eq!(
        missing_status,
        StatusCode::NOT_FOUND,
        "missing detail response: {missing_detail}"
    );
    Ok(())
}
