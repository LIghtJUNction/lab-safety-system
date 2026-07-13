use super::safety_flow_assertions::verify_analytics_and_permissions;
use super::*;

#[tokio::test]
async fn backend_safety_management_flow_is_enforced() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, login) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/auth/password-login",
        None,
        serde_json::json!({
            "username": "admin",
            "password": "AdminStrong123!"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(login["user"]["role"], "system_admin");

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/regulations",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "title": "No permission",
            "regulation_type": "internal",
            "issuing_authority": "Lab",
            "effective_date": "2026-01-01",
            "summary": "researcher cannot create regulations",
            "file_url": null
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let managed_username = format!("managed_{}", ctx.schema);
    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/users",
        Some(&ctx.admin_token),
        serde_json::json!({
            "username": managed_username,
            "display_name": "Managed Researcher",
            "email": format!("{}@example.com", ctx.schema),
            "role": "lab_member",
            "auth_provider": "password",
            "department": "公共实验平台",
            "password": "weak"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, managed_user) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/users",
        Some(&ctx.admin_token),
        serde_json::json!({
            "username": managed_username,
            "display_name": "Managed Researcher",
            "email": format!("{}@example.com", ctx.schema),
            "role": "lab_member",
            "auth_provider": "password",
            "department": "公共实验平台",
            "password": "ManagedStrong123!"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(managed_user["role"], "lab_member");

    let (status, users) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/users?role=lab_member",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(users.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|user| user["username"] == managed_user["username"])
    }));

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/users/{}", managed_user["id"]),
        Some(&ctx.researcher_token),
        serde_json::json!({
            "display_name": "Unauthorized Update"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, updated_user) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/users/{}", managed_user["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({
            "display_name": "Managed Visitor",
            "role": "visitor",
            "is_active": false
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated_user["display_name"], "Managed Visitor");
    assert_eq!(updated_user["role"], "visitor");
    assert_eq!(updated_user["is_active"], false);

    let (status, regulation_upload) = upload(
        &ctx.app,
        "/api/v1/regulations/upload",
        &ctx.admin_token,
        "regulation.txt",
        "wear goggles",
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        regulation_upload["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("/uploads/regulations/"))
    );

    let (status, regulation) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/regulations",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "危险化学品安全管理条例",
            "regulation_type": "国家法规",
            "issuing_authority": "国务院",
            "effective_date": "2026-01-01",
            "summary": "危险化学品采购、储存、使用和处置要求。",
            "file_url": regulation_upload["url"]
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(regulation["file_url"], regulation_upload["url"]);

    let (status, incident_upload) = upload(
        &ctx.app,
        "/api/v1/incidents/upload",
        &ctx.admin_token,
        "incident.txt",
        "incident attachment",
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, lab) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/labs",
        Some(&ctx.admin_token),
        serde_json::json!({
            "code": format!("LAB-{}", ctx.schema),
            "name": "有机化学实验室",
            "location": "实验楼A-302",
            "department": "化学学院",
            "manager_user_id": ctx.researcher_id,
            "contact": "lab@example.com",
            "status": "active",
            "description": "有机合成和试剂暂存实验室"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lab["name"], "有机化学实验室");

    let (status, lab_member) = json_request(
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
    assert_eq!(lab_member["lab_role"], "lab_member");

    let (status, lab_visitor) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/labs/{}/users", lab["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({
            "user_id": managed_user["id"],
            "lab_role": "visitor"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lab_visitor["lab_role"], "visitor");

    let (status, lab_users) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/labs/{}/users", lab["id"]),
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(lab_users.as_array().is_some_and(|items| {
        items
            .iter()
            .any(|item| item["user_id"] == managed_user["id"] && item["lab_role"] == "visitor")
    }));

    let (status, _) = request(
        &ctx.app,
        Method::DELETE,
        &format!("/api/v1/labs/{}/users/{}", lab["id"], managed_user["id"]),
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, incident) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/incidents",
        Some(&ctx.admin_token),
        serde_json::json!({
            "title": "通风橱操作不当事故",
            "lab_id": lab["id"],
            "occurred_on": "2026-05-10",
            "severity": "major",
            "category": "chemical",
            "root_cause": "未按规程开启通风设备",
            "corrective_actions": "重新培训并增加班前检查",
            "file_url": incident_upload["url"]
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(incident["file_url"], incident_upload["url"]);

    let (status, training) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/trainings",
        Some(&ctx.admin_token),
        serde_json::json!({
            "lab_id": lab["id"],
            "title": "化学品入门安全培训",
            "target_role": "lab_member",
            "status": "active",
            "starts_on": "2026-07-01",
            "exam_required_score": 80
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/exam-results",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "training_id": training["id"],
            "user_id": ctx.researcher_id,
            "score": 92
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, equipment) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/equipment",
        Some(&ctx.admin_token),
        serde_json::json!({
            "asset_code": format!("HPLC-{}", ctx.schema),
            "name": "高效液相色谱仪",
            "lab_id": lab["id"],
            "status": "available",
            "owner": "设备管理员"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let booking_payload = serde_json::json!({
        "equipment_id": equipment["id"],
        "user_id": ctx.researcher_id,
        "starts_at": "2026-07-10T02:00:00Z",
        "ends_at": "2026-07-10T04:00:00Z",
        "purpose": "样品检测"
    });
    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/equipment-bookings",
        Some(&ctx.researcher_token),
        booking_payload.clone(),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/equipment-bookings",
        Some(&ctx.researcher_token),
        booking_payload,
    )
    .await?;
    assert_eq!(status, StatusCode::CONFLICT);

    let (status, repair) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/repair-tickets",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "equipment_id": equipment["id"],
            "reported_by": ctx.researcher_id,
            "description": "泵压异常",
            "status": "open"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let (status, closed_repair) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/repair-tickets/{}", repair["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({ "status": "closed" }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed_repair["status"], "closed");

    let (status, labs) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/labs?q=有机",
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        labs.as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == lab["id"]))
    );

    let (status, issue_photo) = upload(
        &ctx.app,
        "/api/v1/hazards/upload/issue-photo",
        &ctx.researcher_token,
        "issue.png",
        "issue photo",
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let (status, hazard) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/hazards",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "title": "试剂柜标签缺失",
            "lab_id": lab["id"],
            "category": "chemical",
            "description": "三号试剂柜部分瓶体缺少中文标签。",
            "reported_by": ctx.researcher_id,
            "issue_photo_url": issue_photo["url"]
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    assert_eq!(hazard["lab_id"], lab["id"]);
    assert_eq!(hazard["lab_name"], lab["name"]);
    // Canonical create status is `open` (not legacy `reported`).
    assert_eq!(hazard["status"], "open");

    // Missing lab_id must fail for multi-lab hazard create.
    let (status, missing_lab) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/hazards",
        Some(&ctx.researcher_token),
        serde_json::json!({
            "title": "no lab",
            "category": "chemical",
            "description": "must bind lab",
            "reported_by": ctx.researcher_id
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        missing_lab["detail"]
            .as_str()
            .is_some_and(|d| d.contains("lab_id")),
        "{missing_lab}"
    );

    // Non-member cannot list or create under another lab.
    let outsider_username = format!("outsider_{}", ctx.schema);
    let (status, outsider) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/users",
        Some(&ctx.admin_token),
        serde_json::json!({
            "username": outsider_username,
            "display_name": "Outsider",
            "email": format!("{}@example.com", outsider_username),
            "role": "lab_member",
            "auth_provider": "password",
            "department": "other",
            "password": "OutsiderStrong123!"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let (status, outsider_login) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/auth/password-login",
        None,
        serde_json::json!({
            "username": outsider_username,
            "password": "OutsiderStrong123!"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let outsider_token = outsider_login["access_token"]
        .as_str()
        .expect("outsider access_token");

    let (status, _) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/hazards?lab_id={}", lab["id"]),
        Some(outsider_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/hazards",
        Some(outsider_token),
        serde_json::json!({
            "title": "intrusion",
            "lab_id": lab["id"],
            "category": "chemical",
            "description": "non-member must not create",
            "reported_by": outsider["id"]
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, lab_hazards) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/hazards?lab_id={}", lab["id"]),
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        lab_hazards
            .as_array()
            .is_some_and(|items| items.iter().any(|item| item["id"] == hazard["id"]))
    );

    let (status, claimed) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/hazards/{}/claim", hazard["id"]),
        Some(&ctx.researcher_token),
        serde_json::json!({ "responsible_user_id": ctx.researcher_id }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(claimed["status"], "claimed");

    let (status, remediation_photo) = upload(
        &ctx.app,
        "/api/v1/hazards/upload/remediation-photo",
        &ctx.researcher_token,
        "remediation.png",
        "fixed photo",
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let (status, remediated) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/hazards/{}/remediation", hazard["id"]),
        Some(&ctx.researcher_token),
        serde_json::json!({
            "remediation_photo_url": remediation_photo["url"],
            "remediation_note": "已补充标签并复核。"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(remediated["status"], "remediation_submitted");

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/hazards/{}/status", hazard["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({ "status": "nonsense" }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/hazards/{}/status", hazard["id"]),
        Some(&ctx.researcher_token),
        serde_json::json!({ "status": "closed" }),
    )
    .await?;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, closed_hazard) = json_request(
        &ctx.app,
        Method::PATCH,
        &format!("/api/v1/hazards/{}/status", hazard["id"]),
        Some(&ctx.admin_token),
        serde_json::json!({ "status": "closed" }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(closed_hazard["status"], "closed");

    verify_analytics_and_permissions(&ctx, &lab, &managed_user).await?;

    Ok(())
}
