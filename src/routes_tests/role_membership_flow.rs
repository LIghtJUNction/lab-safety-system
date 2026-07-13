use super::*;

async fn create_role_user(
    ctx: &TestApp,
    username: &str,
    global_role: &str,
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
    .bind(global_role)
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
async fn lab_roles_and_membership_permissions_should_follow_the_role_matrix() -> anyhow::Result<()>
{
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let lab_one = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'Role lab one', 'active') returning id",
    )
    .bind(format!("ROLE1-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    let lab_two = sqlx::query_scalar::<_, i64>(
        "insert into labs (code, name, status) values ($1, 'Role lab two', 'active') returning id",
    )
    .bind(format!("ROLE2-{}", ctx.schema))
    .fetch_one(&ctx.pool)
    .await?;
    let (lab_admin_id, lab_admin_token) =
        create_role_user(&ctx, "matrix_lab_admin", "lab_member").await?;
    let (lab_member_id, lab_member_token) =
        create_role_user(&ctx, "matrix_lab_member", "lab_member").await?;
    let (visitor_id, visitor_token) = create_role_user(&ctx, "matrix_visitor", "visitor").await?;
    let (target_id, _) = create_role_user(&ctx, "matrix_target", "visitor").await?;
    let (outsider_id, outsider_token) =
        create_role_user(&ctx, "matrix_outsider", "lab_member").await?;

    for (user_id, lab_role) in [
        (lab_admin_id, "lab_admin"),
        (lab_member_id, "lab_member"),
        (visitor_id, "visitor"),
    ] {
        let (status, membership) = json_request(
            &ctx.app,
            Method::POST,
            &format!("/api/v1/labs/{lab_one}/users"),
            Some(&ctx.admin_token),
            serde_json::json!({"user_id": user_id, "lab_role": lab_role}),
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "assign {lab_role}: {membership}");
        assert_eq!(membership["lab_role"], lab_role);
    }

    for (role, expected_status) in [
        ("lab_member", StatusCode::OK),
        ("visitor", StatusCode::OK),
        ("lab_admin", StatusCode::BAD_REQUEST),
        ("system_admin", StatusCode::BAD_REQUEST),
    ] {
        let username = format!("api_{role}_{}", ctx.schema);
        let (status, body) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/users",
            Some(&ctx.admin_token),
            serde_json::json!({
                "username": username,
                "display_name": format!("API {role}"),
                "email": format!("api-{role}-{}@example.com", ctx.schema),
                "role": role,
                "auth_provider": "password",
                "department": "Safety",
                "password": "RoleMatrixStrong123!"
            }),
        )
        .await?;
        assert_eq!(status, expected_status, "create global role {role}: {body}");
    }

    for (label, token) in [
        ("system admin", ctx.admin_token.as_str()),
        ("lab admin", lab_admin_token.as_str()),
        ("lab member", lab_member_token.as_str()),
        ("visitor", visitor_token.as_str()),
    ] {
        let (status, body) = request(
            &ctx.app,
            Method::GET,
            &format!("/api/v1/labs/{lab_one}/users"),
            Some(token),
            Body::empty(),
            None,
        )
        .await?;
        assert_eq!(status, StatusCode::OK, "{label} list own lab: {body}");
    }
    let (outsider_list_status, outsider_list_body) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/labs/{lab_one}/users"),
        Some(&outsider_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        outsider_list_status,
        StatusCode::FORBIDDEN,
        "outsider list: {outsider_list_body}"
    );

    let (admin_assign_status, admin_assign_body) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/labs/{lab_one}/users"),
        Some(&lab_admin_token),
        serde_json::json!({"user_id": target_id, "lab_role": "visitor"}),
    )
    .await?;
    assert_eq!(
        admin_assign_status,
        StatusCode::OK,
        "lab admin assign: {admin_assign_body}"
    );
    let (admin_remove_status, admin_remove_body) = request(
        &ctx.app,
        Method::DELETE,
        &format!("/api/v1/labs/{lab_one}/users/{target_id}"),
        Some(&lab_admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        admin_remove_status,
        StatusCode::NO_CONTENT,
        "lab admin remove: {admin_remove_body}"
    );

    for (label, token) in [
        ("lab member", lab_member_token.as_str()),
        ("visitor", visitor_token.as_str()),
        ("outsider", outsider_token.as_str()),
    ] {
        let (assign_status, assign_body) = json_request(
            &ctx.app,
            Method::POST,
            &format!("/api/v1/labs/{lab_one}/users"),
            Some(token),
            serde_json::json!({"user_id": target_id, "lab_role": "visitor"}),
        )
        .await?;
        assert_eq!(
            assign_status,
            StatusCode::FORBIDDEN,
            "{label} assign: {assign_body}"
        );
    }

    let (foreign_list_status, foreign_list_body) = request(
        &ctx.app,
        Method::GET,
        &format!("/api/v1/labs/{lab_two}/users"),
        Some(&lab_admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        foreign_list_status,
        StatusCode::FORBIDDEN,
        "lab admin foreign list: {foreign_list_body}"
    );
    let (system_assign_status, system_assign_body) = json_request(
        &ctx.app,
        Method::POST,
        &format!("/api/v1/labs/{lab_two}/users"),
        Some(&ctx.admin_token),
        serde_json::json!({"user_id": outsider_id, "lab_role": "lab_member"}),
    )
    .await?;
    assert_eq!(
        system_assign_status,
        StatusCode::OK,
        "system admin foreign assign: {system_assign_body}"
    );
    let (system_remove_status, system_remove_body) = request(
        &ctx.app,
        Method::DELETE,
        &format!("/api/v1/labs/{lab_two}/users/{outsider_id}"),
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(
        system_remove_status,
        StatusCode::NO_CONTENT,
        "system admin foreign remove: {system_remove_body}"
    );
    Ok(())
}
