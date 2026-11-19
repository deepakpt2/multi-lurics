use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::RngCore;
use sha2::{Digest, Sha256};

/// A valid hash of a random password, used to equalize timing when the account does not exist.
static DUMMY_HASH: std::sync::LazyLock<String> =
    std::sync::LazyLock::new(|| hash_password_blocking("dummy-password-for-timing").expect("argon2 works"));

fn hash_password_blocking(plain: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(plain.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))
}

/// Argon2id (crate defaults). CPU-bound, so it runs off the async executor.
pub async fn hash_password(plain: String) -> anyhow::Result<String> {
    tokio::task::spawn_blocking(move || hash_password_blocking(&plain)).await?
}

/// `stored = None` verifies against a dummy hash and always returns false.
pub async fn verify_password(plain: String, stored: Option<String>) -> anyhow::Result<bool> {
    tokio::task::spawn_blocking(move || {
        let (hash, real) = match stored {
            Some(h) => (h, true),
            None => (DUMMY_HASH.clone(), false),
        };
        let parsed = PasswordHash::new(&hash).map_err(|e| anyhow::anyhow!("bad stored hash: {e}"))?;
        Ok(real && Argon2::default().verify_password(plain.as_bytes(), &parsed).is_ok())
    })
    .await?
}

/// 256-bit random token, base64url. Only its SHA-256 is ever persisted.
pub fn new_token() -> (String, [u8; 32]) {
    let mut bytes = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let token = URL_SAFE_NO_PAD.encode(bytes);
    let hash = hash_token(&token);
    (token, hash)
}

pub fn hash_token(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn password_roundtrip_and_dummy_never_matches() {
        let h = hash_password("correct horse battery".into()).await.unwrap();
        assert!(h.starts_with("$argon2id$"));
        assert!(verify_password("correct horse battery".into(), Some(h.clone()))
            .await
            .unwrap());
        assert!(!verify_password("wrong password!!".into(), Some(h)).await.unwrap());
        assert!(!verify_password("dummy-password-for-timing".into(), None).await.unwrap());
    }

    #[test]
    fn tokens_are_unique_and_hash_deterministically() {
        let (a, ha) = new_token();
        let (b, _) = new_token();
        assert_ne!(a, b);
        assert_eq!(a.len(), 43);
        assert_eq!(hash_token(&a), ha);
    }
}
