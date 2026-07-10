use super::*;

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
