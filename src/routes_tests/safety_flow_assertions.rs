use super::*;

pub(super) async fn verify_analytics_and_permissions(
    ctx: &TestApp,
    lab: &serde_json::Value,
    managed_user: &serde_json::Value,
) -> anyhow::Result<()> {
    let (status, lab_dashboard) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/analytics/dashboard?lab_id={}", lab["id"]),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lab_dashboard["incident_count"], 1);
    assert_eq!(lab_dashboard["equipment_count"], 1);

    let (status, lab_incident_analytics) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/analytics/incidents?lab_id={}", lab["id"]),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        lab_incident_analytics["by_category"]
            .as_array()
            .is_some_and(|items| items
                .iter()
                .any(|item| item["name"] == "chemical" && item["count"] == 1))
    );

    let (status, lab_hazard_analytics) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/analytics/hazards?lab_id={}", lab["id"]),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        lab_hazard_analytics["by_status"]
            .as_array()
            .is_some_and(|items| items
                .iter()
                .any(|item| item["name"] == "closed" && item["count"] == 1))
    );

    let (status, _) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/users",
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, promoted_lab_admin) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/labs/{}/users", lab["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({
            "user_id": ctx.researcher_id,
            "lab_role": "lab_admin"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(promoted_lab_admin["lab_role"], "lab_admin");

    let (status, scoped_users) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/users?lab_id={}", lab["id"]),
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let scoped_users = scoped_users.as_array().expect("users array");
    assert!(
        scoped_users
            .iter()
            .any(|user| user["id"] == ctx.researcher_id)
    );
    assert!(
        !scoped_users
            .iter()
            .any(|user| user["id"] == managed_user["id"])
    );

    for path in [
        "/api/v1/regulations?q=危险化学品",
        "/api/v1/incidents",
        "/api/v1/trainings",
        "/api/v1/equipment",
        "/api/v1/equipment-bookings",
        "/api/v1/repair-tickets",
        "/api/v1/hazards",
    ] {
        let (status, value) = request(
            &ctx.app,
            Method::GET,
            path,
            Some(&ctx.researcher_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(
            value.as_array().is_some_and(|items| !items.is_empty()),
            "{path}"
        );
    }

    for path in [
        "/api/v1/analytics/dashboard",
        "/api/v1/analytics/regulations",
        "/api/v1/analytics/incidents",
        "/api/v1/analytics/hazards",
    ] {
        let (status, value) = request(
            &ctx.app,
            Method::GET,
            path,
            Some(&ctx.admin_token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{path}: {value}");
    }

    Ok(())
}
