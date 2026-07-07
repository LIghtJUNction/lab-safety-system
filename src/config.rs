use std::{env, net::SocketAddr, path::PathBuf};

#[derive(Clone, Debug)]
pub struct Settings {
    pub app_env: String,
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
    pub webauthn_rp_id: String,
    pub webauthn_origin: String,
    pub cors_allowed_origins: Vec<String>,
    pub mcp_enabled: bool,
    pub mcp_config: Option<String>,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_env_inner(true)
    }

    pub fn from_env_for_backup() -> anyhow::Result<Self> {
        Self::from_env_inner(false)
    }

    fn from_env_inner(validate_auth_secrets: bool) -> anyhow::Result<Self> {
        let app_env = env::var("APP_ENV").unwrap_or_else(|_| "development".to_string());
        let host = env::var("APP_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
        let port = env::var("APP_PORT").unwrap_or_else(|_| "8080".to_string());
        let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
            "postgresql://lab_safety:change-me@postgres:5432/lab_safety".to_string()
        });
        let secret_key =
            env::var("SECRET_KEY").unwrap_or_else(|_| "change-me-in-production".to_string());
        let sso_enabled = env::var("SSO_ENABLED").is_ok_and(|value| value == "true");
        let oauth_enabled = env::var("OAUTH_ENABLED").is_ok_and(|value| value == "true");
        let federated_login_secret = env::var("FEDERATED_LOGIN_SECRET").ok();
        validate_production_settings(
            &app_env,
            &secret_key,
            &database_url,
            federated_login_secret.as_deref(),
            sso_enabled || oauth_enabled,
            validate_auth_secrets,
        )?;

        Ok(Self {
            app_env,
            bind_addr: format!("{host}:{port}").parse()?,
            database_url,
            secret_key,
            token_ttl_seconds: env::var("TOKEN_TTL_SECONDS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(3600),
            upload_dir: env::var("UPLOAD_DIR")
                .unwrap_or_else(|_| "/app/uploads".to_string())
                .into(),
            static_dir: env::var("STATIC_DIR").ok().map(PathBuf::from),
            sso_enabled,
            oauth_enabled,
            sso_login_url: env::var("SSO_LOGIN_URL").ok(),
            oauth_login_url: env::var("OAUTH_LOGIN_URL").ok(),
            federated_login_secret,
            webauthn_rp_id: env::var("WEBAUTHN_RP_ID").unwrap_or_else(|_| "localhost".to_string()),
            webauthn_origin: env::var("WEBAUTHN_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:5174".to_string()),
            cors_allowed_origins: parse_csv_env("CORS_ALLOWED_ORIGINS"),
            mcp_enabled: env::var("MCP_ENABLED").is_ok_and(|v| v == "true"),
            mcp_config: env::var("MCP_CONFIG").ok(),
        })
    }
}

fn parse_csv_env(key: &str) -> Vec<String> {
    env::var(key)
        .ok()
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn validate_production_settings(
    app_env: &str,
    secret_key: &str,
    database_url: &str,
    federated_login_secret: Option<&str>,
    federated_enabled: bool,
    validate_auth_secrets: bool,
) -> anyhow::Result<()> {
    if app_env != "production" {
        return Ok(());
    }
    if looks_like_placeholder(database_url) {
        anyhow::bail!("DATABASE_URL must not contain placeholder credentials in production");
    }
    if !validate_auth_secrets {
        return Ok(());
    }
    validate_production_secret("SECRET_KEY", secret_key)?;
    if federated_enabled {
        let Some(secret) = federated_login_secret else {
            anyhow::bail!("FEDERATED_LOGIN_SECRET is required when SSO or OAuth is enabled");
        };
        validate_production_secret("FEDERATED_LOGIN_SECRET", secret)?;
    }
    Ok(())
}

fn validate_production_secret(name: &str, value: &str) -> anyhow::Result<()> {
    if value.len() < 32 || looks_like_placeholder(value) {
        anyhow::bail!("{name} must be at least 32 characters and not a placeholder in production");
    }
    Ok(())
}

fn looks_like_placeholder(value: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.contains("change-me") || value.contains("replacewith")
}

#[cfg(test)]
mod tests {
    use super::{parse_csv_env, validate_production_settings};

    #[test]
    fn production_rejects_placeholder_secret() {
        let result = validate_production_settings(
            "production",
            "change-me-in-production",
            "postgresql://lab_safety:strong@postgres:5432/lab_safety",
            None,
            false,
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn production_rejects_placeholder_database_url() {
        let result = validate_production_settings(
            "production",
            "0123456789abcdef0123456789abcdef",
            "postgresql://lab_safety:change-me@postgres:5432/lab_safety",
            None,
            false,
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn production_requires_federated_secret_when_enabled() {
        let result = validate_production_settings(
            "production",
            "0123456789abcdef0123456789abcdef",
            "postgresql://lab_safety:strong@postgres:5432/lab_safety",
            None,
            true,
            true,
        );
        assert!(result.is_err());
    }

    #[test]
    fn development_allows_placeholder_defaults() {
        let result = validate_production_settings(
            "development",
            "change-me-in-production",
            "postgresql://lab_safety:change-me@postgres:5432/lab_safety",
            None,
            false,
            true,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn production_backup_validation_allows_placeholder_auth_secret() {
        let result = validate_production_settings(
            "production",
            "change-me-in-production",
            "postgresql://lab_safety:strong@postgres:5432/lab_safety",
            None,
            false,
            false,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn parse_csv_env_ignores_empty_entries() {
        unsafe {
            std::env::set_var(
                "LAB_SAFETY_TEST_CSV",
                "https://a.example, ,https://b.example,",
            );
        }
        assert_eq!(
            parse_csv_env("LAB_SAFETY_TEST_CSV"),
            vec!["https://a.example", "https://b.example"]
        );
        unsafe {
            std::env::remove_var("LAB_SAFETY_TEST_CSV");
        }
    }
}
