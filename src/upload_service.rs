use std::path::Path;

use axum::{Router, http::StatusCode};
use tower_http::services::ServeDir;

/// Publish business attachments, never the upload root (which also holds backups).
pub(crate) fn public_uploads(upload_dir: &Path) -> Router {
    let mut router = Router::new();
    for category in [
        "regulations",
        "incidents",
        "hazards/issue",
        "hazards/remediation",
    ] {
        router = router.nest_service(
            &format!("/{category}"),
            ServeDir::new(upload_dir.join(category)),
        );
    }
    router.fallback(|| async { StatusCode::NOT_FOUND })
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request},
    };
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn business_attachments_remain_downloadable() {
        let directory = tempfile::tempdir().expect("upload directory");
        for category in [
            "regulations",
            "incidents",
            "hazards/issue",
            "hazards/remediation",
        ] {
            let target = directory.path().join(category);
            std::fs::create_dir_all(&target).expect("category directory");
            std::fs::write(target.join("sample.txt"), "attachment").expect("attachment");
            let app = Router::new().nest_service("/uploads", public_uploads(directory.path()));
            let response = app
                .oneshot(
                    Request::builder()
                        .uri(format!("/uploads/{category}/sample.txt"))
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body"),
                "attachment"
            );
        }
    }

    #[tokio::test]
    async fn backups_and_unpublished_files_are_not_served() {
        let directory = tempfile::tempdir().expect("upload directory");
        std::fs::create_dir_all(directory.path().join("backups")).expect("backup directory");
        std::fs::create_dir_all(directory.path().join("regulations")).expect("document directory");
        std::fs::write(directory.path().join("backups/database.tar.gz"), "private-backup")
            .expect("backup");
        std::fs::write(directory.path().join("database.sql"), "private-database")
            .expect("database");
        let app = Router::new()
            .nest_service("/uploads", public_uploads(directory.path()))
            .fallback(|| async { "spa-index" });
        for path in [
            "/uploads/backups/database.tar.gz",
            "/uploads/%62ackups/database.tar.gz",
            "/uploads/database.sql",
            "/uploads/regulations/../backups/database.tar.gz",
            "/uploads/regulations/%2e%2e/backups/database.tar.gz",
            "/uploads/regulations/%2e%2e%2fbackups%2fdatabase.tar.gz",
        ] {
            for method in [Method::GET, Method::HEAD] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method.clone())
                            .uri(path)
                            .body(Body::empty())
                            .expect("request"),
                    )
                    .await
                    .expect("response");
                assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {path}");
                let body = to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect("body");
                assert!(!String::from_utf8_lossy(&body).contains("private-"));
                assert!(!String::from_utf8_lossy(&body).contains("spa-index"));
            }
        }
    }
}
