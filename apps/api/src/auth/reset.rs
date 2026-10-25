//! Коды и токены восстановления пароля.
//!
//! * Код (6 цифр) в БД не хранится: только HMAC-SHA256 от него, привязанный
//!   к пользователю и секрету сервера. Подбор кода ограничен числом попыток.
//! * Reset-токен - 32 случайных байта (hex). В БД лежит только его SHA-256.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

pub const CODE_LEN: usize = 6;

fn random_u32() -> Result<u32, AppError> {
    getrandom::u32().map_err(|error| {
        tracing::error!(%error, "OS random generator failed");
        AppError::Internal
    })
}

/// Равномерно распределённый код из 6 цифр (с ведущими нулями).
pub fn generate_code() -> Result<String, AppError> {
    // Наибольшее кратное 1_000_000, не превышающее u32::MAX: отбрасываем "хвост",
    // чтобы остаток от деления не давал перекос в сторону маленьких чисел.
    const LIMIT: u32 = 4_294_000_000;

    loop {
        let n = random_u32()?;
        if n < LIMIT {
            return Ok(format!("{:06}", n % 1_000_000));
        }
    }
}

fn code_mac(secret: &str, user_id: Uuid, code: &str) -> HmacSha256 {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(b"rtchesz:reset-code:v1:");
    mac.update(user_id.as_bytes());
    mac.update(code.as_bytes());
    mac
}

pub fn hash_code(secret: &str, user_id: Uuid, code: &str) -> Vec<u8> {
    code_mac(secret, user_id, code)
        .finalize()
        .into_bytes()
        .to_vec()
}

/// Сравнение за постоянное время.
pub fn verify_code(secret: &str, user_id: Uuid, code: &str, expected: &[u8]) -> bool {
    code_mac(secret, user_id, code)
        .verify_slice(expected)
        .is_ok()
}

/// Случайный reset-токен: 64 hex-символа.
pub fn generate_token() -> Result<String, AppError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| {
        tracing::error!(%error, "OS random generator failed");
        AppError::Internal
    })?;

    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

pub fn hash_token(token: &str) -> Vec<u8> {
    Sha256::digest(token.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "unit_test_secret_unit_test_secret_00";

    #[test]
    fn code_is_six_digits() {
        for _ in 0..200 {
            let code = generate_code().unwrap();
            assert_eq!(code.len(), CODE_LEN);
            assert!(code.chars().all(|c| c.is_ascii_digit()), "{code}");
        }
    }

    #[test]
    fn codes_are_not_constant() {
        let codes: std::collections::HashSet<_> =
            (0..50).map(|_| generate_code().unwrap()).collect();
        assert!(codes.len() > 40);
    }

    #[test]
    fn code_hash_is_bound_to_user_code_and_secret() {
        let user = Uuid::new_v4();
        let hash = hash_code(SECRET, user, "123456");

        assert!(verify_code(SECRET, user, "123456", &hash));
        assert!(!verify_code(SECRET, user, "123457", &hash));
        assert!(!verify_code(SECRET, Uuid::new_v4(), "123456", &hash));
        assert!(!verify_code(
            "other_secret_other_secret_other_00",
            user,
            "123456",
            &hash
        ));
        assert!(!verify_code(SECRET, user, "123456", b"short"));
    }

    #[test]
    fn token_format_and_hash() {
        let a = generate_token().unwrap();
        let b = generate_token().unwrap();

        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
        assert_eq!(hash_token(&a), hash_token(&a));
        assert_ne!(hash_token(&a), hash_token(&b));
        assert_eq!(hash_token(&a).len(), 32);
    }
}
