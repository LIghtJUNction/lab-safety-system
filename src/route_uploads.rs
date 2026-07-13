use std::{path::Path, sync::Arc};

use axum::{
    Json,
    extract::{Multipart, State},
    http::HeaderMap,
};
use tokio::fs;
use uuid::Uuid;

use crate::{
    models::UploadedFile,
    route_permissions::require_admin,
    route_support::{ApiError, AppState},
    routes::require_user,
};

const DOCUMENT_MAX_BYTES: usize = 20 * 1024 * 1024;
const PHOTO_MAX_BYTES: usize = 8 * 1024 * 1024;

const DOCUMENT_EXTENSIONS: &[&str] = &["pdf", "doc", "docx", "xls", "xlsx", "csv", "txt", "md"];
const DOCUMENT_CONTENT_TYPES: &[&str] = &[
    "application/pdf",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "text/csv",
    "text/plain",
    "text/markdown",
];
const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
const PHOTO_CONTENT_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp"];

struct UploadPolicy {
    max_bytes: usize,
    extensions: &'static [&'static str],
    content_types: &'static [&'static str],
}

const DOCUMENT_POLICY: UploadPolicy = UploadPolicy {
    max_bytes: DOCUMENT_MAX_BYTES,
    extensions: DOCUMENT_EXTENSIONS,
    content_types: DOCUMENT_CONTENT_TYPES,
};

const PHOTO_POLICY: UploadPolicy = UploadPolicy {
    max_bytes: PHOTO_MAX_BYTES,
    extensions: PHOTO_EXTENSIONS,
    content_types: PHOTO_CONTENT_TYPES,
};

pub(crate) async fn upload_regulation_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    save_upload(&state, multipart, "regulations", &DOCUMENT_POLICY)
        .await
        .map(Json)
}

pub(crate) async fn upload_incident_file(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    let actor = require_user(&state, &headers).await?;
    require_admin(&actor)?;
    save_upload(&state, multipart, "incidents", &DOCUMENT_POLICY)
        .await
        .map(Json)
}

pub(crate) async fn upload_hazard_issue_photo(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    require_user(&state, &headers).await?;
    save_upload(&state, multipart, "hazards/issue", &PHOTO_POLICY)
        .await
        .map(Json)
}

pub(crate) async fn upload_hazard_remediation_photo(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    multipart: Multipart,
) -> Result<Json<UploadedFile>, ApiError> {
    require_user(&state, &headers).await?;
    save_upload(&state, multipart, "hazards/remediation", &PHOTO_POLICY)
        .await
        .map(Json)
}

async fn save_upload(
    state: &AppState,
    mut multipart: Multipart,
    category: &str,
    policy: &UploadPolicy,
) -> Result<UploadedFile, ApiError> {
    let mut upload = None;
    while let Some(field) = multipart.next_field().await? {
        if field.file_name().is_some() {
            upload = Some(field);
            break;
        }
    }
    let Some(field) = upload else {
        return Err(ApiError::bad_request("file field is required"));
    };

    let original_name = field.file_name().unwrap_or("upload.bin").to_string();
    let content_type = field.content_type().map(ToString::to_string);
    let bytes = field.bytes().await?;
    validate_upload(&original_name, content_type.as_deref(), bytes.len(), policy)?;

    let stored_name = format!(
        "{}-{}",
        Uuid::new_v4(),
        Path::new(&original_name)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("upload.bin")
    );
    let target_dir = state.settings.upload_dir.join(category);
    fs::create_dir_all(&target_dir).await?;
    fs::write(target_dir.join(&stored_name), &bytes).await?;

    Ok(UploadedFile {
        filename: original_name,
        size: bytes.len(),
        url: format!("/uploads/{category}/{stored_name}"),
        content_type,
    })
}

fn validate_upload(
    filename: &str,
    content_type: Option<&str>,
    size: usize,
    policy: &UploadPolicy,
) -> Result<(), ApiError> {
    if size == 0 {
        return Err(ApiError::bad_request("Uploaded file is empty"));
    }
    if size > policy.max_bytes {
        return Err(ApiError::bad_request("Uploaded file is too large"));
    }

    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| ApiError::bad_request("Uploaded file extension is required"))?;
    if !policy.extensions.contains(&extension.as_str()) {
        return Err(ApiError::bad_request("Uploaded file type is not allowed"));
    }

    if let Some(content_type) = content_type {
        let content_type = content_type
            .split(';')
            .next()
            .unwrap_or(content_type)
            .trim()
            .to_ascii_lowercase();
        if !content_type.is_empty() && !policy.content_types.contains(&content_type.as_str()) {
            return Err(ApiError::bad_request(
                "Uploaded file content type is not allowed",
            ));
        }
    }

    Ok(())
}
