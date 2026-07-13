use super::*;

fn bucket_sum(value: &serde_json::Value, field: &str) -> i64 {
    value[field]
        .as_array()
        .expect("bucket array")
        .iter()
        .map(|bucket| bucket["count"].as_i64().expect("bucket count"))
        .sum()
}

#[tokio::test]
async fn analytics_dashboard_and_bucket_sums_should_match_deterministic_data() -> anyhow::Result<()>
{
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let lab_one = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'Analytics one', 'active') returning id",
    )
    .bind(format!("AN1-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    let lab_two = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'Analytics two', 'active') returning id",
    )
    .bind(format!("AN2-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    sqlx::query("delete from regulations")
        .execute(&ctx.pool)
        .await?;
    sqlx::query("insert into lab_users (lab_id, user_id, lab_role) values ($1, $2, 'lab_member')")
        .bind(lab_one)
        .bind(ctx.researcher_id)
        .execute(&ctx.pool)
        .await?;

    sqlx::query(
        r#"
        insert into regulations (title, regulation_type, issuing_authority, summary)
        values
            ('Analytics regulation A', 'internal', 'Safety office', 'A'),
            ('Analytics regulation B', 'national', 'Ministry', 'B')
        "#,
    )
    .execute(&ctx.pool)
    .await?;
    sqlx::query(
        r#"
        insert into incident_cases (
            title, lab_id, lab_name, occurred_on, severity, category,
            root_cause, corrective_actions
        )
        values
            ('Lab one chemical', $1, 'Analytics one', '2026-07-01', 'major', 'chemical', 'A', 'A'),
            ('Lab one equipment', $1, 'Analytics one', '2026-07-02', 'minor', 'equipment', 'B', 'B'),
            ('Lab two excluded', $2, 'Analytics two', '2026-07-03', 'major', 'chemical', 'C', 'C')
        "#,
    )
    .bind(lab_one)
    .bind(lab_two)
    .execute(&ctx.pool)
    .await?;
    let training_ids = sqlx::query_scalar::<_, i64>(
        r#"
        insert into trainings (lab_id, title, target_role, status, exam_required_score)
        values
            ($1, 'Analytics training A', 'lab_member', 'active', 80),
            ($1, 'Analytics training B', 'lab_member', 'active', 80)
        returning id
        "#,
    )
    .bind(lab_one)
    .fetch_all(&ctx.pool)
    .await?;
    sqlx::query(
        r#"
        insert into exam_results (training_id, user_id, score, status)
        values ($1, $3, 90, 'passed'), ($2, $3, 70, 'failed')
        "#,
    )
    .bind(training_ids[0])
    .bind(training_ids[1])
    .bind(ctx.researcher_id)
    .execute(&ctx.pool)
    .await?;
    let equipment_ids = sqlx::query_scalar::<_, i64>(
        r#"
        insert into equipment (asset_code, name, lab_id, lab_name, status)
        values
            ($1, 'Analytics equipment A', $3, 'Analytics one', 'available'),
            ($2, 'Analytics equipment B', $3, 'Analytics one', 'maintenance')
        returning id
        "#,
    )
    .bind(format!("ANA-{}", ctx.schema))
    .bind(format!("ANB-{}", ctx.schema))
    .bind(lab_one)
    .fetch_all(&ctx.pool)
    .await?;
    sqlx::query(
        r#"
        insert into repair_tickets (equipment_id, reported_by, description, status)
        values ($1, $3, 'Open repair', 'open'), ($2, $3, 'Closed repair', 'closed')
        "#,
    )
    .bind(equipment_ids[0])
    .bind(equipment_ids[1])
    .bind(ctx.researcher_id)
    .execute(&ctx.pool)
    .await?;
    sqlx::query(
        r#"
        insert into safety_hazards (
            title, lab_id, lab_name, category, description, status, reported_by
        )
        values
            ('Hazard open', $1, 'Analytics one', 'chemical', 'A', 'open', $3),
            ('Hazard claimed', $1, 'Analytics one', 'chemical', 'B', 'claimed', $3),
            ('Hazard closed', $1, 'Analytics one', 'equipment', 'C', 'closed', $3),
            ('Hazard excluded', $2, 'Analytics two', 'chemical', 'D', 'open', $3)
        "#,
    )
    .bind(lab_one)
    .bind(lab_two)
    .bind(ctx.researcher_id)
    .execute(&ctx.pool)
    .await?;

    let (dashboard_status, dashboard) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/analytics/dashboard?lab_id={lab_one}"),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(dashboard_status, StatusCode::OK, "dashboard: {dashboard}");
    assert_eq!(
        dashboard,
        serde_json::json!({
            "regulation_count": 2,
            "incident_count": 2,
            "training_count": 2,
            "equipment_count": 2,
            "open_repair_count": 1,
            "exam_pass_rate": 0.5
        })
    );

    let (incident_status, incident) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/analytics/incidents?lab_id={lab_one}"),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        incident_status,
        StatusCode::OK,
        "incident analytics: {incident}"
    );
    assert_eq!(bucket_sum(&incident, "by_category"), 2);
    assert_eq!(bucket_sum(&incident, "by_severity"), 2);

    let (hazard_status, hazard) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/analytics/hazards?lab_id={lab_one}"),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(hazard_status, StatusCode::OK, "hazard analytics: {hazard}");
    assert_eq!(bucket_sum(&hazard, "by_status"), 3);
    assert_eq!(bucket_sum(&hazard, "by_category"), 3);

    let (regulation_status, regulation) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/analytics/regulations",
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        regulation_status,
        StatusCode::OK,
        "regulation analytics: {regulation}"
    );
    assert_eq!(bucket_sum(&regulation, "by_type"), 2);
    assert_eq!(bucket_sum(&regulation, "by_authority"), 2);
    Ok(())
}
