use super::*;

#[tokio::test]
async fn hazard_lifecycle_should_record_history_and_preserve_reopen_evidence() -> anyhow::Result<()>
{
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let lab_id = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'History lab', 'active') returning id",
    )
    .bind(format!("HISTORY-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    sqlx::query("insert into lab_users (lab_id, user_id, lab_role) values ($1, $2, 'lab_member')")
        .bind(lab_id)
        .bind(ctx.researcher_id)
        .execute(&ctx.pool)
        .await?;
    sqlx::query(
        r#"
        insert into users (username, display_name, email, role, auth_provider)
        values ('history_outsider', 'History outsider', 'history-out@example.com', 'lab_member', 'password')
        "#,
    )
    .execute(&ctx.pool)
    .await?;
    let outsider_token = crate::security::create_access_token(
        "history_outsider",
        &ctx.settings.secret_key,
        ctx.settings.token_ttl_seconds,
    )?;

    let (status, created) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/hazards",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "title": "Lifecycle history",
            "lab_id": lab_id,
            "category": "chemical",
            "description": "Record every transition",
            "reported_by": ctx.researcher_id,
            "issue_photo_url": "/uploads/hazards/issue/evidence.jpg"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "create response: {created}");
    assert_eq!(created["status"], "open");
    let hazard_id = created["id"].as_i64().expect("hazard id");

    let (status, claimed) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/hazards/{hazard_id}/claim"),
        Some(&ctx.researcher_token),
        serde_json::json!({"responsible_user_id": ctx.researcher_id}),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "claim response: {claimed}");
    assert_eq!(claimed["status"], "claimed");

    let remediation_photo = "/uploads/hazards/remediation/remediated.jpg";
    let remediation_note = "Installed secondary containment";
    let (status, remediated) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/hazards/{hazard_id}/remediation"),
        Some(&ctx.researcher_token),
        serde_json::json!({
            "remediation_photo_url": remediation_photo,
            "remediation_note": remediation_note
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "remediation response: {remediated}");
    assert_eq!(remediated["status"], "remediation_submitted");

    let (status, closed) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/hazards/{hazard_id}/status"),
        Some(&ctx.admin_token),
        serde_json::json!({"status": "closed"}),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "close response: {closed}");
    assert_eq!(closed["status"], "closed");

    let (status, reopened) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/hazards/{hazard_id}/status"),
        Some(&ctx.admin_token),
        serde_json::json!({"status": "remediation_submitted"}),
    )
    .await?;
    assert_eq!(status, StatusCode::OK, "reopen response: {reopened}");
    assert_eq!(reopened["status"], "remediation_submitted");
    assert_eq!(reopened["responsible_user_id"], ctx.researcher_id);
    assert_eq!(reopened["remediation_photo_url"], remediation_photo);
    assert_eq!(reopened["remediation_note"], remediation_note);

    let (illegal_status, illegal_body) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/hazards/{hazard_id}/status"),
        Some(&ctx.admin_token),
        serde_json::json!({"status": "open"}),
    )
    .await?;
    assert_eq!(
        illegal_status,
        StatusCode::CONFLICT,
        "illegal transition response: {illegal_body}"
    );

    let (history_status, history) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/hazards/{hazard_id}/history"),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        history_status,
        StatusCode::OK,
        "history response: {history}"
    );
    let transitions = history
        .as_array()
        .expect("history array")
        .iter()
        .map(|event| {
            (
                event["from_status"].as_str().map(ToOwned::to_owned),
                event["to_status"].as_str().expect("to_status").to_owned(),
                event["actor_user_id"].as_i64(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        transitions,
        vec![
            (None, "open".to_owned(), Some(ctx.researcher_id)),
            (
                Some("open".to_owned()),
                "claimed".to_owned(),
                Some(ctx.researcher_id)
            ),
            (
                Some("claimed".to_owned()),
                "remediation_submitted".to_owned(),
                Some(ctx.researcher_id)
            ),
            (
                Some("remediation_submitted".to_owned()),
                "closed".to_owned(),
                Some(ctx.admin_id)
            ),
            (
                Some("closed".to_owned()),
                "remediation_submitted".to_owned(),
                Some(ctx.admin_id)
            ),
        ]
    );
    let (outsider_status, outsider_body) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/hazards/{hazard_id}/history"),
        Some(&outsider_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        outsider_status,
        StatusCode::FORBIDDEN,
        "outsider history response: {outsider_body}"
    );
    Ok(())
}
