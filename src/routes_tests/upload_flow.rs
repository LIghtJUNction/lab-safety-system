use super::*;

async fn upload_as(
    app: &Router,
    path: &str,
    token: &str,
    filename: &str,
    content_type: &str,
) -> anyhow::Result<(StatusCode, serde_json::Value)> {
    upload_bytes_as(app, path, token, filename, content_type, b"content", false).await
}

async fn upload_bytes_as(
    app: &Router,
    path: &str,
    token: &str,
    filename: &str,
    content_type: &str,
    content: &[u8],
    regular_field_first: bool,
) -> anyhow::Result<(StatusCode, serde_json::Value)> {
    let boundary = "x-test-boundary-custom";
    let mut body = Vec::new();
    if regular_field_first {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"description\"\r\n\r\nmetadata\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: {content_type}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(content);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
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
async fn document_upload_accepts_large_file_after_regular_field() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let content = vec![b'a'; 3 * 1024 * 1024];

    let (status, uploaded) = upload_bytes_as(
        &ctx.app,
        "/api/v1/regulations/upload",
        &ctx.admin_token,
        "large.txt",
        "text/plain",
        &content,
        true,
    )
    .await?;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(uploaded["size"].as_u64(), Some(content.len() as u64));
    Ok(())
}

#[tokio::test]
async fn photo_upload_accepts_file_above_default_body_limit() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let content = vec![0_u8; 3 * 1024 * 1024];

    let (status, uploaded) = upload_bytes_as(
        &ctx.app,
        "/api/v1/hazards/upload/issue-photo",
        &ctx.researcher_token,
        "issue.png",
        "image/png",
        &content,
        false,
    )
    .await?;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(uploaded["size"].as_u64(), Some(content.len() as u64));
    Ok(())
}

#[tokio::test]
async fn document_upload_rejects_file_over_business_limit() -> anyhow::Result<()> {
    let Some(ctx) = test_app().await? else {
        return Ok(());
    };
    let content = vec![b'a'; 20 * 1024 * 1024 + 1];

    let (status, body) = upload_bytes_as(
        &ctx.app,
        "/api/v1/regulations/upload",
        &ctx.admin_token,
        "too-large.txt",
        "text/plain",
        &content,
        false,
    )
    .await?;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["detail"], "Uploaded file is too large");
    Ok(())
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
