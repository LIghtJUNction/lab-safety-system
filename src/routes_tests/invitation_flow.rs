use super::*;

#[tokio::test]
async fn invitation_registration_enforces_limits_and_lab_role() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, lab) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/labs",
        Some(&ctx.admin_token),
        serde_json::json!({
            "code": format!("INV-LAB-{}", ctx.schema),
            "name": "Invitation validation lab",
            "status": "active"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/invitations",
        Some(&ctx.admin_token),
        serde_json::json!({
            "lab_id": lab["id"],
            "target_role": "lab_member",
            "max_uses": 0
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/invitations",
        Some(&ctx.admin_token),
        serde_json::json!({
            "lab_id": lab["id"],
            "target_role": "owner",
            "max_uses": 1
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, invitation) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/invitations",
        Some(&ctx.admin_token),
        serde_json::json!({
            "lab_id": lab["id"],
            "target_role": "lab_member",
            "max_uses": 1,
            "memo": "single-use link"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    let invite_code = invitation["code"].as_str().expect("invitation code string");

    let (status, public_info) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/invitations/public/{invite_code}"),
        None,
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(public_info["target_role"], "lab_member");
    assert_eq!(public_info["lab_name"], lab["name"]);

    let invited_username = format!("invited_{}", ctx.schema);
    let (status, registered) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/invitations/register",
        None,
        serde_json::json!({
            "code": invite_code,
            "username": invited_username,
            "display_name": "Invited Member",
            "email": format!("invited-{}@example.com", ctx.schema),
            "password": "InvitedStrong123!"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(registered["username"], invited_username);

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
    assert!(lab_users.as_array().is_some_and(|users| {
        users
            .iter()
            .any(|user| user["username"] == invited_username && user["lab_role"] == "lab_member")
    }));

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/invitations/register",
        None,
        serde_json::json!({
            "code": invite_code,
            "username": format!("second_{}", ctx.schema),
            "display_name": "Second Member",
            "email": format!("second-{}@example.com", ctx.schema),
            "password": "SecondStrong123!"
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    Ok(())
}
