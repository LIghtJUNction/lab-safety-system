use super::*;

#[tokio::test]
async fn auth_settings_patch_rejects_empty_environment_secret_when_enabling_provider()
-> anyhow::Result<()> {
    let Some(ctx) = test_app_with_federated_secret(Some("   ")).await? else {
        return Ok(());
    };

    let (_, settings) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(settings["federated_login_secret_configured"], false);

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn auth_settings_patch_rejects_short_environment_secret_when_enabling_provider()
-> anyhow::Result<()> {
    let Some(ctx) = test_app_with_federated_secret(Some("short-secret")).await? else {
        return Ok(());
    };

    let (_, settings) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(settings["federated_login_secret_configured"], false);

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn auth_settings_get_rejects_non_admin() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, _) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/settings/auth",
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;

    assert_eq!(status, StatusCode::FORBIDDEN);
    Ok(())
}

#[tokio::test]
async fn auth_settings_get_returns_environment_defaults_without_secret() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, body) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["sso_enabled"], false);
    assert!(body["sso_login_url"].is_null());
    assert_eq!(body["oauth_enabled"], false);
    assert!(body["oauth_login_url"].is_null());
    assert_eq!(body["federated_login_secret_configured"], false);
    assert!(body.get("federated_login_secret").is_none());
    Ok(())
}

#[tokio::test]
async fn auth_settings_patch_rejects_enabling_provider_without_secret() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn auth_settings_patch_rejects_non_http_login_url() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "javascript:alert(1)",
            "oauth_enabled": false,
            "oauth_login_url": null,
            "federated_login_secret": "federated-settings-test-secret-32-chars"
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    Ok(())
}

#[tokio::test]
async fn auth_settings_patch_updates_auth_methods_immediately() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let secret = "federated-settings-test-secret-32-chars";

    let (status, body) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": true,
            "oauth_login_url": "https://idp.example/oauth/authorize",
            "federated_login_secret": secret
        }),
    )
    .await?;

    assert_eq!(status, StatusCode::OK, "{body:?}");
    assert_eq!(body["federated_login_secret_configured"], true);
    assert!(body.get("federated_login_secret").is_none());

    let (methods_status, methods) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/auth/methods",
        None,
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(methods_status, StatusCode::OK);
    assert_eq!(methods["sso"], true);
    assert_eq!(methods["oauth"], true);
    assert_eq!(methods["sso_login_url"], "https://idp.example/sso/login");
    assert_eq!(
        methods["oauth_login_url"],
        "https://idp.example/oauth/authorize"
    );
    Ok(())
}

#[tokio::test]
async fn auth_settings_storage_encrypts_secret_and_reload_restores_it() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let secret = "reloadable-federated-settings-secret-32-chars";

    let (status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null,
            "federated_login_secret": secret
        }),
    )
    .await?;
    assert_eq!(status, StatusCode::OK);

    let stored: serde_json::Value =
        sqlx::query_scalar("select value from site_settings where key = 'federated_auth'")
            .fetch_one(&ctx.pool)
            .await?;
    assert!(!stored.to_string().contains(secret));
    assert!(stored.get("federated_login_secret").is_none());
    assert!(
        stored["encrypted_federated_login_secret"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );

    let reloaded = crate::auth_settings::load(&ctx.pool, &ctx.settings).await?;
    assert_eq!(reloaded.federated_login_secret.as_deref(), Some(secret));
    assert!(reloaded.sso_enabled);
    Ok(())
}

#[tokio::test]
async fn auth_settings_clear_requires_disabled_providers_and_removes_secret() -> anyhow::Result<()>
{
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let secret = "clearable-federated-settings-secret-32-chars";

    let (seed_status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null,
            "federated_login_secret": secret
        }),
    )
    .await?;
    assert_eq!(seed_status, StatusCode::OK);

    let (conflict_status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null,
            "clear_federated_login_secret": true
        }),
    )
    .await?;
    assert_eq!(conflict_status, StatusCode::BAD_REQUEST);

    let (clear_status, cleared) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": false,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null,
            "clear_federated_login_secret": true
        }),
    )
    .await?;
    assert_eq!(clear_status, StatusCode::OK);
    assert_eq!(cleared["federated_login_secret_configured"], false);

    let reloaded = crate::auth_settings::load(&ctx.pool, &ctx.settings).await?;
    assert!(reloaded.federated_login_secret.is_none());
    assert!(!reloaded.sso_enabled);
    Ok(())
}

#[tokio::test]
async fn auth_settings_load_fails_when_encrypted_secret_is_corrupted() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    sqlx::query(
        r#"
        insert into site_settings (key, value)
        values ('federated_auth', $1)
        on conflict (key) do update set value = excluded.value
        "#,
    )
    .bind(serde_json::json!({
        "sso_enabled": true,
        "sso_login_url": "https://idp.example/sso/login",
        "oauth_enabled": false,
        "oauth_login_url": null,
        "encrypted_federated_login_secret": "not-valid-encrypted-data"
    }))
    .execute(&ctx.pool)
    .await?;

    let error = crate::auth_settings::load(&ctx.pool, &ctx.settings)
        .await
        .expect_err("corrupt encrypted settings must fail startup loading");
    assert!(error.to_string().contains("cannot be decrypted"));
    Ok(())
}

#[tokio::test]
async fn deployment_settings_are_admin_only_and_expose_only_safe_fields() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (forbidden, _) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/settings/deployment",
        Some(&ctx.researcher_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(forbidden, StatusCode::FORBIDDEN);

    let (status, body) = request(
        &ctx.app,
        Method::GET,
        "/api/v1/settings/deployment",
        Some(&ctx.admin_token),
        Body::empty(),
        None,
    )
    .await?;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["app_env"], "test");
    assert_eq!(body["token_ttl_seconds"], 3600);
    assert_eq!(body["webauthn_rp_id"], "localhost");
    assert_eq!(body["webauthn_origin"], "http://localhost:5174");
    assert_eq!(body["cors_allowed_origins"], serde_json::json!([]));
    assert_eq!(body["mcp_enabled"], true);
    assert_eq!(
        body["callback_paths"],
        serde_json::json!({
            "sso": "/api/v1/auth/sso/callback",
            "oauth": "/api/v1/auth/oauth/callback"
        })
    );
    for sensitive in [
        "database_url",
        "secret_key",
        "upload_dir",
        "static_dir",
        "mcp_config",
    ] {
        assert!(
            body.get(sensitive).is_none(),
            "unexpected field {sensitive}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn dynamic_auth_settings_enable_user_creation_and_callback_verification() -> anyhow::Result<()>
{
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let secret = "dynamic-provider-settings-secret-32-chars";
    let (patch_status, _) = json_request(
        &ctx.app,
        Method::PATCH,
        "/api/v1/settings/auth",
        Some(&ctx.admin_token),
        serde_json::json!({
            "sso_enabled": true,
            "sso_login_url": "https://idp.example/sso/login",
            "oauth_enabled": false,
            "oauth_login_url": null,
            "federated_login_secret": secret
        }),
    )
    .await?;
    assert_eq!(patch_status, StatusCode::OK);

    let created_username = format!("dynamic_created_{}", ctx.schema);
    let (create_status, _) = json_request(
        &ctx.app,
        Method::POST,
        "/api/v1/users",
        Some(&ctx.admin_token),
        serde_json::json!({
            "username": created_username,
            "display_name": "Dynamic Created",
            "email": format!("dynamic-created-{}@example.com", ctx.schema),
            "role": "lab_member",
            "auth_provider": "sso"
        }),
    )
    .await?;
    assert_eq!(create_status, StatusCode::OK);

    let callback_username = format!("dynamic_callback_{}", ctx.schema);
    let callback_email = format!("dynamic-callback-{}@example.com", ctx.schema);
    let display_name = "DynamicCallback";
    let exp = chrono::Utc::now().timestamp() + 300;
    let message = crate::route_support::federated_signature_message(
        "sso",
        &callback_username,
        &callback_email,
        display_name,
        "lab_member",
        "",
        exp,
    );
    let signature = crate::security::sign_message(&message, secret)?;
    let path = format!(
        "/api/v1/auth/sso/callback?username={callback_username}&email={callback_email}&display_name={display_name}&role=lab_member&exp={exp}&sig={signature}"
    );
    let (callback_status, _) =
        request(&ctx.app, Method::GET, &path, None, Body::empty(), None).await?;
    assert_eq!(callback_status, StatusCode::OK);

    let provider: String =
        sqlx::query_scalar("select auth_provider from users where username = $1")
            .bind(callback_username)
            .fetch_one(&ctx.pool)
            .await?;
    assert_eq!(provider, "sso");
    Ok(())
}
