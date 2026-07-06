use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Settings {
    pub bind_addr: SocketAddr,
    pub database_url: String,
    pub secret_key: String,
    pub token_ttl_seconds: i64,
    pub upload_dir: PathBuf,
    pub static_dir: Option<PathBuf>,
    pub sso_enabled: bool,
    pub oauth_enabled: bool,
    pub sso_login_url: Option<String>,
    pub oauth_login_url: Option<String>,
    pub federated_login_secret: Option<String>,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let host = env::var("APP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("APP_PORT").unwrap_or_else(|_| "8080".to_string());
        Ok(Self {
            bind_addr: format!("{host}:{port}").parse()?,
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgresql://lab_safety:change-me@postgres:5432/lab_safety".to_string()
            }),
            secret_key: env::var("SECRET_KEY")
                .unwrap_or_else(|_| "change-me-in-production".to_string()),
            token_ttl_seconds: env::var("TOKEN_TTL_SECONDS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(3600),
            upload_dir: env::var("UPLOAD_DIR")
                .unwrap_or_else(|_| "/app/uploads".to_string())
                .into(),
            static_dir: env::var("STATIC_DIR").ok().map(PathBuf::from),
            sso_enabled: env::var("SSO_ENABLED").is_ok_and(|value| value == "true"),
            oauth_enabled: env::var("OAUTH_ENABLED").is_ok_and(|value| value == "true"),
            sso_login_url: env::var("SSO_LOGIN_URL").ok(),
            oauth_login_url: env::var("OAUTH_LOGIN_URL").ok(),
            federated_login_secret: env::var("FEDERATED_LOGIN_SECRET").ok(),
        })
    }
}
