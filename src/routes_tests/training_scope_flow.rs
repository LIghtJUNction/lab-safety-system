use super::*;

async fn create_scoped_user(
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

async fn create_lab(ctx: &TestApp, code: &str, name: &str) -> anyhow::Result<i64> {
    Ok(sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, $2, 'active') returning id",
    )
    .bind(code)
    .bind(name)
    .fetch_one(&ctx.pool)
    .await?)
}

async fn assign_lab_role(
    ctx: &TestApp,
    lab_id: i64,
    user_id: i64,
    lab_role: &str,
) -> anyhow::Result<()> {
    sqlx::query("insert into lab_users (lab_id, user_id, lab_role) values ($1, $2, $3)")
        .bind(lab_id)
        .bind(user_id)
        .bind(lab_role)
        .execute(&ctx.pool)
        .await?;
    Ok(())
}

#[tokio::test]
async fn trainings_should_require_real_lab_and_enforce_manager_and_reader_scope()
-> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let lab_one = create_lab(&ctx, &format!("TR1-{}", ctx.schema), "Training lab one").await?;
    let lab_two = create_lab(&ctx, &format!("TR2-{}", ctx.schema), "Training lab two").await?;
    let (lab_admin_id, lab_admin_token) =
        create_scoped_user(&ctx, "training_lab_admin", "lab_member").await?;
    assign_lab_role(&ctx, lab_one, lab_admin_id, "lab_admin").await?;
    assign_lab_role(&ctx, lab_one, ctx.researcher_id, "lab_member").await?;

    let training_payload = |lab_id: i64, title: &str| {
        serde_json::json!({
            "lab_id": lab_id,
            "title": title,
            "target_role": "lab_member",
            "status": "active",
            "starts_on": "2026-07-13",
            "exam_required_score": 80
        })
    };
    let (status, lab_one_training) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/trainings",
        Some(&lab_admin_token),
        training_payload(lab_one, "Lab one training"),
    )
    .await?;
    assert_eq!(
        status,
        StatusCode::OK,
        "lab admin create response: {lab_one_training}"
    );
    assert_eq!(lab_one_training["lab_id"], lab_one);

    let (foreign_create_status, foreign_create_body) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/trainings",
        Some(&lab_admin_token),
        training_payload(lab_two, "Forbidden training"),
    )
    .await?;
    assert_eq!(
        foreign_create_status,
        StatusCode::FORBIDDEN,
        "foreign create response: {foreign_create_body}"
    );

    let (status, lab_two_training) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/trainings",
        Some(&ctx.admin_token),
        training_payload(lab_two, "Lab two training"),
    )
    .await?;
    assert_eq!(
        status,
        StatusCode::OK,
        "system admin create: {lab_two_training}"
    );

    let (member_status, member_trainings) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/trainings?lab_id={lab_one}"),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        member_status,
        StatusCode::OK,
        "member list: {member_trainings}"
    );
    assert_eq!(member_trainings.as_array().map(Vec::len), Some(1));
    assert_eq!(member_trainings[0]["id"], lab_one_training["id"]);

    let (foreign_list_status, foreign_list_body) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/trainings?lab_id={lab_two}"),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        foreign_list_status,
        StatusCode::FORBIDDEN,
        "foreign list response: {foreign_list_body}"
    );

    let (unscoped_status, unscoped_body) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/trainings",
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        unscoped_status,
        StatusCode::FORBIDDEN,
        "unscoped list response: {unscoped_body}"
    );
    Ok(())
}

#[tokio::test]
async fn exam_results_and_dashboard_should_follow_training_lab_scope() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let lab_one = create_lab(&ctx, &format!("EX1-{}", ctx.schema), "Exam lab one").await?;
    let lab_two = create_lab(&ctx, &format!("EX2-{}", ctx.schema), "Exam lab two").await?;
    assign_lab_role(&ctx, lab_one, ctx.researcher_id, "lab_member").await?;
    let (other_user_id, _) = create_scoped_user(&ctx, "exam_other_user", "lab_member").await?;
    let (visitor_id, visitor_token) = create_scoped_user(&ctx, "exam_visitor", "visitor").await?;
    assign_lab_role(&ctx, lab_one, visitor_id, "visitor").await?;

    let training_one = sqlx::query_scalar::<_, i64>(
        r#"
        insert into trainings (lab_id, title, target_role, status, exam_required_score)
        values ($1, 'Exam one', 'lab_member', 'active', 80)
        returning id
        "#,
    )
    .bind(lab_one)
    .fetch_one(&ctx.pool)
    .await?;
    let training_two = sqlx::query_scalar::<_, i64>(
        r#"
        insert into trainings (lab_id, title, target_role, status, exam_required_score)
        values ($1, 'Exam two', 'lab_member', 'active', 70)
        returning id
        "#,
    )
    .bind(lab_one)
    .fetch_one(&ctx.pool)
    .await?;
    let foreign_training = sqlx::query_scalar::<_, i64>(
        r#"
        insert into trainings (lab_id, title, target_role, status, exam_required_score)
        values ($1, 'Foreign exam', 'lab_member', 'active', 80)
        returning id
        "#,
    )
    .bind(lab_two)
    .fetch_one(&ctx.pool)
    .await?;
    let legacy_training = sqlx::query_scalar::<_, i64>(
        r#"
        insert into trainings (title, target_role, status, exam_required_score)
        values ('Legacy global exam', 'lab_member', 'active', 80)
        returning id
        "#,
    )
    .fetch_one(&ctx.pool)
    .await?;

    let (self_status, self_result) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/exam-results",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "training_id": training_one,
            "user_id": ctx.researcher_id,
            "score": 90
        }),
    )
    .await?;
    assert_eq!(self_status, StatusCode::OK, "self result: {self_result}");
    assert_eq!(self_result["status"], "passed");

    let (visitor_status, visitor_body) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/exam-results",
        Some(&visitor_token),
        serde_json::json!({
            "training_id": training_one,
            "user_id": visitor_id,
            "score": 90
        }),
    )
    .await?;
    assert_eq!(
        visitor_status,
        StatusCode::FORBIDDEN,
        "visitor exam result: {visitor_body}"
    );

    let (other_status, other_body) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/exam-results",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "training_id": training_two,
            "user_id": other_user_id,
            "score": 60
        }),
    )
    .await?;
    assert_eq!(
        other_status,
        StatusCode::FORBIDDEN,
        "other-user result: {other_body}"
    );

    let (foreign_status, foreign_body) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/exam-results",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "training_id": foreign_training,
            "user_id": ctx.researcher_id,
            "score": 90
        }),
    )
    .await?;
    assert_eq!(
        foreign_status,
        StatusCode::FORBIDDEN,
        "foreign result: {foreign_body}"
    );

    for (training_id, user_id, score) in [
        (training_two, other_user_id, 60),
        (foreign_training, ctx.researcher_id, 90),
    ] {
        let (status, body) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/exam-results",
            Some(&ctx.admin_token),
            serde_json::json!({
                "training_id": training_id,
                "user_id": user_id,
                "score": score
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "admin result: {body}");
    }

    let (legacy_status, legacy_body) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/exam-results",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "training_id": legacy_training,
            "user_id": ctx.researcher_id,
            "score": 90
        }),
    )
    .await?;
    assert_eq!(
        legacy_status,
        StatusCode::FORBIDDEN,
        "legacy result: {legacy_body}"
    );

    let (lab_dashboard_status, lab_dashboard) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/analytics/dashboard?lab_id={lab_one}"),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        lab_dashboard_status,
        StatusCode::OK,
        "lab dashboard: {lab_dashboard}"
    );
    assert_eq!(lab_dashboard["training_count"], 2);
    assert_eq!(lab_dashboard["exam_pass_rate"], 0.5);

    let (global_dashboard_status, global_dashboard) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/analytics/dashboard",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        global_dashboard_status,
        StatusCode::OK,
        "global dashboard: {global_dashboard}"
    );
    assert_eq!(global_dashboard["training_count"], 4);
    let global_pass_rate = global_dashboard["exam_pass_rate"]
        .as_f64()
        .expect("global pass rate");
    assert!((global_pass_rate - (2.0 / 3.0)).abs() < f64::EPSILON);

    let (global_list_status, global_trainings) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/trainings",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        global_list_status,
        StatusCode::OK,
        "global list: {global_trainings}"
    );
    assert!(global_trainings.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|training| training["id"] == legacy_training && training["lab_id"].is_null())
    }));
    Ok(())
}
