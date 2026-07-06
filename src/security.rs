use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::{distributions::Alphanumeric, Rng};
use serde_json::json;
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn hash_password(password: &str) -> String {
    let salt: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();
    let mut output = [0u8; 32];
    pbkdf2_hmac::<Sha256>(password.as_bytes(), salt.as_bytes(), 210_000, &mut output);
    format!("pbkdf2_sha256${salt}${}", hex(&output))
}

pub fn validate_password_strength(password: &str) -> Result<(), String> {
    if password.len() < 12 {
        return Err("Password must be at least 12 characters long".into());
    }
    if !password.chars().any(|value| value.is_ascii_lowercase()) {
        return Err("Password must contain a lowercase letter".into());
    }
    if !password.chars().any(|value| value.is_ascii_uppercase()) {
        return Err("Password must contain an uppercase letter".into());
    }
    if !password.chars().any(|value| value.is_ascii_digit()) {
        return Err("Password must contain a digit".into());
    }
    if !password.chars().any(|value| !value.is_ascii_alphanumeric()) {
        return Err("Password must contain a symbol".into());
    }
    Ok(())
}

pub fn verify_password(password: &str, stored: Option<&str>) -> bool {
    let Some(stored) = stored else { return false };
    let parts: Vec<_> = stored.split('$').collect();
    if parts.len() != 3 || parts[0] != "pbkdf2_sha256" {
        return false;
    }
    let mut output = [0u8; 32];
    pbkdf2_hmac::<Sha256>(
        password.as_bytes(),
        parts[1].as_bytes(),
        210_000,
        &mut output,
    );
    hex(&output) == parts[2]
}

pub fn create_access_token(
    subject: &str,
    secret: &str,
    ttl_seconds: i64,
) -> anyhow::Result<String> {
    let exp = chrono::Utc::now().timestamp() + ttl_seconds;
    let body = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({ "sub": subject, "exp": exp }))?);
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(body.as_bytes());
    let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    Ok(format!("{body}.{signature}"))
}

fn hex(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push_str(&format!("{byte:02x}"));
    }
    value
}

#[cfg(test)]
mod tests {
    use super::{hash_password, validate_password_strength, verify_password};

    #[test]
    fn password_hash_round_trips() {
        let hash = hash_password("ChangeMe123!");
        assert!(verify_password("ChangeMe123!", Some(&hash)));
        assert!(!verify_password("wrong", Some(&hash)));
    }

    #[test]
    fn password_strength_rejects_weak_values() {
        assert!(validate_password_strength("weak").is_err());
        assert!(validate_password_strength("longbutnosymbol1A").is_err());
        assert!(validate_password_strength("StrongPassw0rd!").is_ok());
    }
}
