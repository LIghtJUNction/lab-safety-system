use super::*;

async fn upload_as(
    app: &Router,
    path: &str,
    token: &str,
    filename: &str,
    content_type: &str,
) -> anyhow::Result<(StatusCode, serde_json::Value)> {
    let boundary = "x-test-boundary-custom";
    let body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\ncontent\r\n--{boundary}--\r\n"
    );
    request(
        app,
        Method::POST,
        path,
        Some(token),
        Body::from(body),
        Some(&format!("multipart/form-data; boundary={boundary}")),
    )
    .await
}

#[tokio::test]
async fn uploads_reject_wrong_file_types() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };

    let (status, _) = upload_as(
        &ctx.app,
        "/api/v1/hazards/upload/issue-photo",
        &ctx.researcher_token,
        "issue.txt",
        "text/plain",
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = upload_as(
        &ctx.app,
        "/api/v1/hazards/upload/issue-photo",
        &ctx.researcher_token,
        "issue.png",
        "text/plain",
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, _) = upload_as(
        &ctx.app,
        "/api/v1/regulations/upload",
        &ctx.admin_token,
        "tool.exe",
        "application/octet-stream",
    )
    .await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    Ok(())
}
