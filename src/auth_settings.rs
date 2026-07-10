use anyhow::{Context, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

use crate::config::Settings;

const SETTINGS_KEY: &str = "federated_auth";
const NONCE_LENGTH: usize = 24;
pub(crate) const MIN_FEDERATED_SECRET_LENGTH: usize = 32;

#[derive(Clone, Debug)]
pub(crate) struct AuthRuntimeSettings {
    pub(crate) sso_enabled: bool,
    pub(crate) sso_login_url: Option<String>,
    pub(crate) oauth_enabled: bool,
    pub(crate) oauth_login_url: Option<String>,
    pub(crate) federated_login_secret: Option<String>,
}

impl AuthRuntimeSettings {
    pub(crate) fn from_settings(settings: &Settings) -> Self {
        Self {
            sso_enabled: settings.sso_enabled,
            sso_login_url: settings.sso_login_url.clone(),
            oauth_enabled: settings.oauth_enabled,
            oauth_login_url: settings.oauth_login_url.clone(),
            federated_login_secret: normalize_federated_secret(
                settings.federated_login_secret.as_deref(),
            ),
        }
    }

    pub(crate) fn valid_federated_login_secret(&self) -> Option<&str> {
        valid_federated_secret(self.federated_login_secret.as_deref())
    }
}

pub(crate) fn normalize_federated_secret(value: Option<&str>) -> Option<String> {
    valid_federated_secret(value).map(ToString::to_string)
}

fn valid_federated_secret(value: Option<&str>) -> Option<&str> {
    value
        .map(str::trim)
        .filter(|secret| secret.len() >= MIN_FEDERATED_SECRET_LENGTH)
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct PersistedAuthSettings {
    sso_enabled: bool,
    sso_login_url: Option<String>,
    oauth_enabled: bool,
    oauth_login_url: Option<String>,
    encrypted_federated_login_secret: Option<String>,
}

pub(crate) async fn load(
    pool: &PgPool,
    environment: &Settings,
) -> anyhow::Result<AuthRuntimeSettings> {
    let value = sqlx::query_scalar::<_, serde_json::Value>(
        "select value from site_settings where key = $1 limit 1",
    )
    .bind(SETTINGS_KEY)
    .fetch_optional(pool)
    .await?;
    let Some(value) = value else {
        return Ok(AuthRuntimeSettings::from_settings(environment));
    };

    let persisted: PersistedAuthSettings = serde_json::from_value(value)
        .context("stored federated authentication settings are invalid")?;
    persisted.into_runtime(&environment.secret_key)
}

pub(crate) async fn save(
    pool: &PgPool,
    runtime: &AuthRuntimeSettings,
    secret_key: &str,
) -> anyhow::Result<()> {
    let persisted = PersistedAuthSettings::from_runtime(runtime, secret_key)?;
    let value = serde_json::to_value(persisted)?;
    sqlx::query(
        r#"
        insert into site_settings (key, value, updated_at)
        values ($1, $2, now())
        on conflict (key) do update set value = excluded.value, updated_at = now()
        "#,
    )
    .bind(SETTINGS_KEY)
    .bind(value)
    .execute(pool)
    .await?;
    Ok(())
}

impl PersistedAuthSettings {
    fn from_runtime(runtime: &AuthRuntimeSettings, secret_key: &str) -> anyhow::Result<Self> {
        Ok(Self {
            sso_enabled: runtime.sso_enabled,
            sso_login_url: runtime.sso_login_url.clone(),
            oauth_enabled: runtime.oauth_enabled,
            oauth_login_url: runtime.oauth_login_url.clone(),
            encrypted_federated_login_secret: runtime
                .federated_login_secret
                .as_deref()
                .map(|secret| encrypt_secret(secret, secret_key))
                .transpose()?,
        })
    }

    fn into_runtime(self, secret_key: &str) -> anyhow::Result<AuthRuntimeSettings> {
        let decrypted_secret = self
            .encrypted_federated_login_secret
            .as_deref()
            .map(|encrypted| decrypt_secret(encrypted, secret_key))
            .transpose()
            .context("stored federated login secret cannot be decrypted")?;
        Ok(AuthRuntimeSettings {
            sso_enabled: self.sso_enabled,
            sso_login_url: self.sso_login_url,
            oauth_enabled: self.oauth_enabled,
            oauth_login_url: self.oauth_login_url,
            federated_login_secret: normalize_federated_secret(decrypted_secret.as_deref()),
        })
    }
}

fn encrypt_secret(secret: &str, secret_key: &str) -> anyhow::Result<String> {
    let key = derive_key(secret_key);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let mut nonce_bytes = [0_u8; NONCE_LENGTH];
    rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::try_from(nonce_bytes.as_slice())
        .map_err(|_| anyhow!("failed to construct federated login secret nonce"))?;
    let ciphertext = cipher
        .encrypt(&nonce, secret.as_bytes())
        .map_err(|_| anyhow!("failed to encrypt federated login secret"))?;
    let mut encoded = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
    encoded.extend_from_slice(&nonce_bytes);
    encoded.extend_from_slice(&ciphertext);
    Ok(STANDARD.encode(encoded))
}

fn decrypt_secret(encrypted: &str, secret_key: &str) -> anyhow::Result<String> {
    let encoded = STANDARD
        .decode(encrypted)
        .context("encrypted federated login secret is not valid base64")?;
    if encoded.len() <= NONCE_LENGTH {
        return Err(anyhow!("encrypted federated login secret is truncated"));
    }
    let (nonce_bytes, ciphertext) = encoded.split_at(NONCE_LENGTH);
    let key = derive_key(secret_key);
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = XNonce::try_from(nonce_bytes)
        .map_err(|_| anyhow!("encrypted federated login secret nonce is invalid"))?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| anyhow!("encrypted federated login secret authentication failed"))?;
    String::from_utf8(plaintext).context("decrypted federated login secret is not UTF-8")
}

fn derive_key(secret_key: &str) -> [u8; 32] {
    Sha256::digest(secret_key.as_bytes()).into()
}
