use argon2::{
    Argon2,
    password_hash::{PasswordHasher, PasswordVerifier, phc::PasswordHash},
};

use crate::error::AppError;

/// Хэширует пароль Argon2id со случайной солью. Результат - PHC-строка.
pub fn hash(password: &str) -> Result<String, AppError> {
    let hash: PasswordHash = Argon2::default()
        .hash_password(password.as_bytes())
        .map_err(|error| {
            tracing::error!(%error, "failed to hash password");
            AppError::Internal
        })?;

    Ok(hash.to_string())
}

/// Проверяет пароль по PHC-строке. Любая ошибка разбора трактуется как "не совпало".
pub fn verify(password: &str, phc_hash: &str) -> bool {
    let Ok(parsed) = PasswordHash::new(phc_hash) else {
        return false;
    };

    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Argon2 намеренно тяжёлый, поэтому считаем его вне асинхронного рантайма.
pub async fn hash_async(password: String) -> Result<String, AppError> {
    tokio::task::spawn_blocking(move || hash(&password))
        .await
        .map_err(|error| {
            tracing::error!(%error, "password hashing task failed");
            AppError::Internal
        })?
}

pub async fn verify_async(password: String, phc_hash: String) -> bool {
    tokio::task::spawn_blocking(move || verify(&password, &phc_hash))
        .await
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash("correct horse battery staple").unwrap();

        assert!(hash.starts_with("$argon2id$"));
        assert!(verify("correct horse battery staple", &hash));
        assert!(!verify("wrong password", &hash));
    }

    #[test]
    fn salts_are_random() {
        assert_ne!(
            hash("same password").unwrap(),
            hash("same password").unwrap()
        );
    }

    #[test]
    fn garbage_hash_never_verifies() {
        assert!(!verify("password", "not a phc string"));
        assert!(!verify("password", ""));
    }
}
