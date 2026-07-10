use super::*;
use axum::response::IntoResponse;

fn user_payload(username: &str, email: &str) -> serde_json::Value {
    serde_json::json!({
        "username": username,
        "display_name": "Duplicate User Test",
        "email": email,
        "role": "lab_member",
        "auth_provider": "password",
        "department": "Safety",
        "password": "DuplicateUser123!"
    })
}

#[tokio::test]
async fn internal_error_detail_should_not_expose_source_message() {
    let sensitive = "internal filesystem path /secret/data";
    let response = ApiError::from(std::io::Error::other(sensitive)).into_response();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read API error body");
    let json: serde_json::Value = serde_json::from_slice(&body).expect("parse API error JSON");

    assert_eq!(json["detail"], "An internal server error occurred");
    assert!(!String::from_utf8_lossy(&body).contains(sensitive));
}

fn assert_sqlx_mapping(
    error: sqlx::Error,
    expected_code: &str,
    expected_status: StatusCode,
    expected_message: &str,
) {
    let actual_code = error
        .as_database_error()
        .and_then(|database_error| database_error.code())
        .map(|code| code.into_owned());
    assert_eq!(actual_code.as_deref(), Some(expected_code));

    let api_error = ApiError::from(error);
    assert_eq!(api_error.status, expected_status);
    assert_eq!(api_error.message, expected_message);
}

#[tokio::test]
async fn not_null_violation_should_map_to_required_value_bad_request() -> anyhow::Result<()> {
    let Some(test) = test_app().await? else {
        return Ok(());
    };
    sqlx::query("create table error_not_null (required_value text not null)")
        .execute(&test.pool)
        .await?;
    let error = sqlx::query("insert into error_not_null (required_value) values (null)")
        .execute(&test.pool)
        .await
        .expect_err("PostgreSQL should reject a null required value");

    assert_sqlx_mapping(
        error,
        "23502",
        StatusCode::BAD_REQUEST,
        "A required value is missing",
    );
    Ok(())
}

#[tokio::test]
async fn foreign_key_violation_should_map_to_reference_conflict() -> anyhow::Result<()> {
    let Some(test) = test_app().await? else {
        return Ok(());
    };
    sqlx::query("create table error_parent (id bigint primary key)")
        .execute(&test.pool)
        .await?;
    sqlx::query("create table error_child (parent_id bigint references error_parent(id))")
        .execute(&test.pool)
        .await?;
    let error = sqlx::query("insert into error_child (parent_id) values (999)")
        .execute(&test.pool)
        .await
        .expect_err("PostgreSQL should reject a missing foreign key target");

    assert_sqlx_mapping(
        error,
        "23503",
        StatusCode::CONFLICT,
        "The referenced record does not exist or is still in use",
    );
    Ok(())
}

#[tokio::test]
async fn check_violation_should_map_to_constraint_bad_request() -> anyhow::Result<()> {
    let Some(test) = test_app().await? else {
        return Ok(());
    };
    sqlx::query("create table error_check (positive_value integer check (positive_value > 0))")
        .execute(&test.pool)
        .await?;
    let error = sqlx::query("insert into error_check (positive_value) values (0)")
        .execute(&test.pool)
        .await
        .expect_err("PostgreSQL should reject a failed check constraint");

    assert_sqlx_mapping(
        error,
        "23514",
        StatusCode::BAD_REQUEST,
        "A value does not satisfy the required constraints",
    );
    Ok(())
}

#[tokio::test]
async fn non_user_unique_violation_should_map_to_generic_conflict() -> anyhow::Result<()> {
    let Some(test) = test_app().await? else {
        return Ok(());
    };
    sqlx::query("create table error_unique (code text unique)")
        .execute(&test.pool)
        .await?;
    sqlx::query("insert into error_unique (code) values ('duplicate')")
        .execute(&test.pool)
        .await?;
    let error = sqlx::query("insert into error_unique (code) values ('duplicate')")
        .execute(&test.pool)
        .await
        .expect_err("PostgreSQL should reject a duplicate non-user value");

    assert_sqlx_mapping(
        error,
        "23505",
        StatusCode::CONFLICT,
        "A record with the same unique value already exists",
    );
    Ok(())
}

#[tokio::test]
async fn duplicate_username_should_return_stable_conflict_detail() {
    let test = match test_app().await.expect("create user error test app") {
        Some(test) => test,
        None => {
            eprintln!("skipping duplicate user integration test: TEST_DATABASE_URL is not set");
            return;
        }
    };
    let (first_status, _) = json_request(
        &test.app,
        Method::POST,
        "/api/v1/users",
        Some(&test.admin_token),
        user_payload("duplicate_user", "duplicate-user-1@example.com"),
    )
    .await
    .expect("first user creation");
    assert_eq!(first_status, StatusCode::OK);

    let (second_status, second_body) = json_request(
        &test.app,
        Method::POST,
        "/api/v1/users",
        Some(&test.admin_token),
        user_payload("duplicate_user", "duplicate-user-2@example.com"),
    )
    .await
    .expect("duplicate user creation response");

    assert_eq!(second_status, StatusCode::CONFLICT);
    assert_eq!(
        second_body["detail"],
        "A user with this username already exists"
    );
}

#[tokio::test]
async fn duplicate_email_should_return_distinct_conflict_detail() {
    let test = match test_app().await.expect("create user error test app") {
        Some(test) => test,
        None => {
            eprintln!("skipping duplicate user integration test: TEST_DATABASE_URL is not set");
            return;
        }
    };
    let email = "duplicate-email@example.com";

    let (first_status, _) = json_request(
        &test.app,
        Method::POST,
        "/api/v1/users",
        Some(&test.admin_token),
        user_payload("duplicate_email_first", email),
    )
    .await
    .expect("first user creation");
    assert_eq!(first_status, StatusCode::OK);

    let (second_status, second_body) = json_request(
        &test.app,
        Method::POST,
        "/api/v1/users",
        Some(&test.admin_token),
        user_payload("duplicate_email_second", email),
    )
    .await
    .expect("duplicate email response");

    assert_eq!(second_status, StatusCode::CONFLICT);
    assert_eq!(
        second_body["detail"],
        "A user with this email already exists"
    );
}
