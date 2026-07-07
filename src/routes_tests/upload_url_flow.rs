use super::*;

#[tokio::test]
async fn create_routes_accept_only_uploaded_local_urls() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/regulations",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "External regulation URL",
            "regulation_type": "internal",
            "issuing_authority": "Lab",
            "effective_date": "2026-01-01",
            "summary": "external URL should not bypass upload checks",
            "file_url": "https://example.com/regulation.pdf"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, regulation) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/regulations",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "Uploaded regulation URL",
            "regulation_type": "internal",
            "issuing_authority": "Lab",
            "effective_date": "2026-01-01",
            "summary": "local uploaded URL is allowed",
            "file_url": "/uploads/regulations/regulation.pdf"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        regulation["file_url"],
        "/uploads/regulations/regulation.pdf"
    );

    let (status, lab) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/labs",
        Some(&ctx.admin_token),
        serde_json::json!({
            "code": format!("URL-LAB-{}", ctx.schema),
            "name": "Upload URL lab",
            "status": "active"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
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
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/incidents",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "External incident URL",
            "lab_id": lab["id"],
            "occurred_on": "2026-05-10",
            "severity": "medium",
            "category": "chemical",
            "root_cause": "procedure gap",
            "corrective_actions": "update checklist",
            "file_url": "/uploads/regulations/wrong-place.pdf"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, incident) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/incidents",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "Uploaded incident URL",
            "lab_id": lab["id"],
            "occurred_on": "2026-05-10",
            "severity": "medium",
            "category": "chemical",
            "root_cause": "procedure gap",
            "corrective_actions": "update checklist",
            "file_url": "/uploads/incidents/incident.pdf"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(incident["file_url"], "/uploads/incidents/incident.pdf");

    Ok(())
}

#[tokio::test]
async fn hazard_routes_accept_only_uploaded_photo_urls() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, lab) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/labs",
        Some(&ctx.admin_token),
        serde_json::json!({
            "code": format!("HZ-URL-LAB-{}", ctx.schema),
            "name": "Hazard URL lab",
            "status": "active"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
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
    assert_eq!(status, StatusCode::OK);

    let hazard_payload = serde_json::json!({
        "title": "Spill near sink",
        "lab_id": lab["id"],
        "category": "chemical",
        "description": "small spill near sink",
        "reported_by": ctx.researcher_id
    });

    let mut rejected_hazard = hazard_payload.clone();
    rejected_hazard["issue_photo_url"] =
        serde_json::json!("/uploads/hazards/remediation/wrong-place.png");
    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/hazards",
        Some(&ctx.researcher_token),
        rejected_hazard,
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let mut accepted_hazard = hazard_payload;
    accepted_hazard["issue_photo_url"] = serde_json::json!("/uploads/hazards/issue/issue.png");
    let (status, hazard) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/hazards",
        Some(&ctx.researcher_token),
        accepted_hazard,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        hazard["issue_photo_url"],
        "/uploads/hazards/issue/issue.png"
    );

    let (status, claimed) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/hazards/{}/claim", hazard["id"]),
        Some(&ctx.researcher_token),
        serde_json::json!({
            "responsible_user_id": ctx.researcher_id
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(claimed["status"], "claimed");

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/hazards/{}/remediation", hazard["id"]),
        Some(&ctx.researcher_token),
        serde_json::json!({
            "remediation_photo_url": "https://example.com/remediation.png",
            "remediation_note": "cleaned"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, remediated) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/hazards/{}/remediation", hazard["id"]),
        Some(&ctx.researcher_token),
        serde_json::json!({
            "remediation_photo_url": "/uploads/hazards/remediation/remediation.png",
            "remediation_note": "cleaned"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        remediated["remediation_photo_url"],
        "/uploads/hazards/remediation/remediation.png"
    );

    Ok(())
}
