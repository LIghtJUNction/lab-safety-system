use super::*;

#[tokio::test]
async fn user_creation_rejects_disabled_federated_providers() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    for auth_provider in ["sso", "oauth"] {
        let (status, _) = json_request(
            &ctx.app,
            Method::POST,
            "/api/v1/users",
            Some(&ctx.admin_token),
            serde_json::json!({
                "username": format!("{}_{}", auth_provider, ctx.schema),
                "display_name": format!("{auth_provider} disabled user"),
                "email": format!("{}-{}@example.com", auth_provider, ctx.schema),
                "role": "lab_member",
                "auth_provider": auth_provider,
                "department": "公共实验平台"
            }),
        )
        .await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    Ok(())
}

#[tokio::test]
async fn password_login_rejects_federated_users_even_with_password_hash() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let username = format!("sso_password_{}", ctx.schema);
    let password = "FederatedStrong123!";
    let password_hash = hash_password(password);
    sqlx::query(
        r#"
        insert into users (username, display_name, email, role, auth_provider, password_hash)
        values ($1, 'Federated User', $2, 'lab_member', 'sso', $3)
        "#,
    )
    .bind(&username)
    .bind(format!("{username}@example.com"))
    .bind(password_hash)
    .execute(&ctx.pool)
    .await?;

    let (status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/auth/password-login",
        None,
        serde_json::json!({
            "username": username,
            "password": password
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    Ok(())
}
