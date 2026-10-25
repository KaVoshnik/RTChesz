use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// ID пользователя.
    sub: String,
    iat: u64,
    exp: u64,
}

/// Выпуск и проверка JWT (HS256).
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
    validation: Validation,
    ttl: Duration,
}

impl JwtKeys {
    pub fn new(secret: &str, ttl: Duration) -> Self {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = 5;
        validation.set_required_spec_claims(&["exp", "sub"]);

        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
            validation,
            ttl,
        }
    }

    pub fn issue(&self, user_id: Uuid) -> Result<String, AppError> {
        let now = now_secs();
        let claims = Claims {
            sub: user_id.to_string(),
            iat: now,
            exp: now + self.ttl.as_secs(),
        };

        encode(&Header::new(Algorithm::HS256), &claims, &self.encoding).map_err(|error| {
            tracing::error!(%error, "failed to sign jwt");
            AppError::Internal
        })
    }

    /// Возвращает ID пользователя из валидного токена.
    pub fn verify(&self, token: &str) -> Result<Uuid, AppError> {
        let invalid = || AppError::unauthorized("Недействительный или просроченный токен");

        let data =
            decode::<Claims>(token, &self.decoding, &self.validation).map_err(|_| invalid())?;

        Uuid::parse_str(&data.claims.sub).map_err(|_| invalid())
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "unit_test_secret_unit_test_secret_00";

    #[test]
    fn issue_and_verify_roundtrip() {
        let keys = JwtKeys::new(SECRET, Duration::from_secs(60));
        let id = Uuid::new_v4();

        let token = keys.issue(id).unwrap();
        assert_eq!(keys.verify(&token).unwrap(), id);
    }

    #[test]
    fn rejects_token_signed_with_other_secret() {
        let ours = JwtKeys::new(SECRET, Duration::from_secs(60));
        let theirs = JwtKeys::new(
            "another_secret_another_secret_0000",
            Duration::from_secs(60),
        );

        let token = theirs.issue(Uuid::new_v4()).unwrap();
        assert!(ours.verify(&token).is_err());
    }

    #[test]
    fn rejects_tampered_and_garbage_tokens() {
        let keys = JwtKeys::new(SECRET, Duration::from_secs(60));
        let mut token = keys.issue(Uuid::new_v4()).unwrap();
        token.push('x');

        assert!(keys.verify(&token).is_err());
        assert!(keys.verify("not.a.jwt").is_err());
        assert!(keys.verify("").is_err());
    }

    #[test]
    fn rejects_expired_token() {
        let keys = JwtKeys::new(SECRET, Duration::from_secs(60));
        let claims = Claims {
            sub: Uuid::new_v4().to_string(),
            iat: now_secs() - 1000,
            exp: now_secs() - 500,
        };
        let token = encode(&Header::new(Algorithm::HS256), &claims, &keys.encoding).unwrap();

        assert!(keys.verify(&token).is_err());
    }
}
