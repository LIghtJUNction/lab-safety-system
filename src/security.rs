use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use hmac::{Hmac, Mac};
use pbkdf2::pbkdf2_hmac;
use rand::{distributions::Alphanumeric, Rng};
use serde::Deserialize;
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

pub fn verify_access_token(token: &str, secret: &str) -> anyhow::Result<String> {
    let Some((body, signature)) = token.split_once('.') else {
        anyhow::bail!("Invalid token");
    };
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(body.as_bytes());
    let expected = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());
    if expected != signature {
        anyhow::bail!("Invalid token signature");
    }
    let payload: TokenPayload = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(body)?)?;
    if payload.exp < chrono::Utc::now().timestamp() {
        anyhow::bail!("Token expired");
    }
    Ok(payload.sub)
}

pub fn sign_message(message: &str, secret: &str) -> anyhow::Result<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())?;
    mac.update(message.as_bytes());
    Ok(URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub fn verify_message_signature(message: &str, signature: &str, secret: &str) -> bool {
    let Ok(expected) = sign_message(message, secret) else {
        return false;
    };
    expected == signature
}

#[derive(Deserialize)]
struct TokenPayload {
    sub: String,
    exp: i64,
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
    use super::{
        create_access_token, hash_password, sign_message, validate_password_strength,
        verify_access_token, verify_message_signature, verify_password,
    };

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

    #[test]
    fn access_token_round_trips_and_rejects_wrong_secret() {
        let token = create_access_token("admin", "secret-a", 60).unwrap();
        assert_eq!(verify_access_token(&token, "secret-a").unwrap(), "admin");
        assert!(verify_access_token(&token, "secret-b").is_err());
    }

    #[test]
    fn message_signature_round_trips() {
        let signature = sign_message("provider|user@example.com", "secret").unwrap();
        assert!(verify_message_signature(
            "provider|user@example.com",
            &signature,
            "secret"
        ));
        assert!(!verify_message_signature(
            "provider|other@example.com",
            &signature,
            "secret"
        ));
    }
}
