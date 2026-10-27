//! JWT (только HS256), реализованный напрямую на `hmac` + `sha2`.
//!
//! Зачем не готовая библиотека: нам нужен один алгоритм, а `jsonwebtoken` тянет за собой
//! RSA и эллиптические кривые (`rsa`, `p256`, `p384`), в которых есть открытые уязвимости
//! (RUSTSEC-2023-0071 и др.), хотя мы их не используем. Здесь алгоритм зашит жёстко:
//! токен с любым другим `alg` (в том числе `none`) отвергается.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use uuid::Uuid;

use crate::error::AppError;

type HmacSha256 = Hmac<Sha256>;

/// `{"typ":"JWT","alg":"HS256"}` - тот же заголовок, что выпускала прежняя библиотека.
const HEADER: &str = r#"{"typ":"JWT","alg":"HS256"}"#;

/// Запас на расхождение часов, секунды.
const LEEWAY_SECS: u64 = 5;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    /// ID пользователя.
    sub: String,
    iat: u64,
    exp: u64,
    /// Версия токенов пользователя на момент выпуска (см. `users.token_version`).
    /// У токенов, выпущенных до появления этого поля, его нет - считаем 0.
    #[serde(default)]
    ver: i32,
}

#[derive(Deserialize)]
struct JwtHeader {
    alg: String,
}

/// Результат проверки подписи и срока токена.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedToken {
    pub user_id: Uuid,
    pub version: i32,
}

/// Выпуск и проверка JWT (HS256).
pub struct JwtKeys {
    secret: Vec<u8>,
    ttl: Duration,
}

impl JwtKeys {
    pub fn new(secret: &str, ttl: Duration) -> Self {
        Self {
            secret: secret.as_bytes().to_vec(),
            ttl,
        }
    }

    fn mac(&self) -> HmacSha256 {
        HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts keys of any length")
    }

    pub fn issue(&self, user_id: Uuid, version: i32) -> Result<String, AppError> {
        let now = now_secs();

        self.sign(&Claims {
            sub: user_id.to_string(),
            iat: now,
            exp: now + self.ttl.as_secs(),
            ver: version,
        })
    }

    fn sign(&self, claims: &Claims) -> Result<String, AppError> {
        let payload = serde_json::to_vec(claims).map_err(|error| {
            tracing::error!(%error, "failed to serialize jwt claims");
            AppError::Internal
        })?;

        let signing_input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(HEADER),
            URL_SAFE_NO_PAD.encode(payload)
        );

        let mut mac = self.mac();
        mac.update(signing_input.as_bytes());
        let signature = URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes());

        Ok(format!("{signing_input}.{signature}"))
    }

    /// Проверяет подпись и срок. Совпадение версии с БД проверяет вызывающий код (`AuthUser`).
    pub fn verify(&self, token: &str) -> Result<VerifiedToken, AppError> {
        self.verify_at(token, now_secs())
    }

    fn verify_at(&self, token: &str, now: u64) -> Result<VerifiedToken, AppError> {
        let invalid = || AppError::unauthorized("Недействительный или просроченный токен");

        let mut parts = token.split('.');
        let (Some(header), Some(payload), Some(signature), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid());
        };

        // Алгоритм зашит жёстко: `alg` из токена только сверяем, но не выбираем по нему проверку.
        let header_json = URL_SAFE_NO_PAD.decode(header).map_err(|_| invalid())?;
        let header: JwtHeader = serde_json::from_slice(&header_json).map_err(|_| invalid())?;
        if header.alg != "HS256" {
            return Err(invalid());
        }

        // Подпись считается по исходным строкам сегментов; сравнение - за постоянное время.
        let (signing_input, _) = token.rsplit_once('.').ok_or_else(invalid)?;
        let signature = URL_SAFE_NO_PAD.decode(signature).map_err(|_| invalid())?;
        let mut mac = self.mac();
        mac.update(signing_input.as_bytes());
        mac.verify_slice(&signature).map_err(|_| invalid())?;

        let payload = URL_SAFE_NO_PAD.decode(payload).map_err(|_| invalid())?;
        let claims: Claims = serde_json::from_slice(&payload).map_err(|_| invalid())?;

        if now > claims.exp.saturating_add(LEEWAY_SECS) {
            return Err(invalid());
        }

        let user_id = Uuid::parse_str(&claims.sub).map_err(|_| invalid())?;

        Ok(VerifiedToken {
            user_id,
            version: claims.ver,
        })
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
    const USER: &str = "9123a087-742b-4a3c-8f1e-0a1b2c3d4e5f";

    // Токены, собранные независимой реализацией (Python hmac), exp - 2100 год.
    // Заголовок в старом формате и без `ver` - как выдавала прежняя библиотека.
    const LEGACY_NO_VER: &str = "eyJ0eXAiOiJKV1QiLCJhbGciOiJIUzI1NiJ9.eyJzdWIiOiI5MTIzYTA4Ny03NDJiLTRhM2MtOGYxZS0wYTFiMmMzZDRlNWYiLCJpYXQiOjE3MDAwMDAwMDAsImV4cCI6NDEwMjQ0NDgwMH0.hCSOqxrN4RuWFO5IKYNuvmRjbnIcI_m15O6QHA8LvYI";
    const WITH_VER_7: &str = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiI5MTIzYTA4Ny03NDJiLTRhM2MtOGYxZS0wYTFiMmMzZDRlNWYiLCJpYXQiOjE3MDAwMDAwMDAsImV4cCI6NDEwMjQ0NDgwMCwidmVyIjo3fQ.uPpu5q0WbGrpNfMgGrQebVGDSrx4zjrUmwNIIWAzzTo";
    const ALG_NONE: &str = "eyJhbGciOiJub25lIn0.eyJzdWIiOiAiOTEyM2EwODctNzQyYi00YTNjLThmMWUtMGExYjJjM2Q0ZTVmIiwgImV4cCI6IDQxMDI0NDQ4MDB9.";

    fn keys() -> JwtKeys {
        JwtKeys::new(SECRET, Duration::from_secs(60))
    }

    #[test]
    fn issue_and_verify_roundtrip() {
        let keys = keys();
        let id = Uuid::new_v4();

        let token = keys.issue(id, 3).unwrap();
        let verified = keys.verify(&token).unwrap();

        assert_eq!(verified.user_id, id);
        assert_eq!(verified.version, 3);
    }

    #[test]
    fn accepts_tokens_from_an_independent_implementation() {
        let id = Uuid::parse_str(USER).unwrap();

        // Старый формат без `ver` читается как версия 0: сессии переживают смену библиотеки.
        assert_eq!(
            keys().verify(LEGACY_NO_VER).unwrap(),
            VerifiedToken {
                user_id: id,
                version: 0
            }
        );
        assert_eq!(
            keys().verify(WITH_VER_7).unwrap(),
            VerifiedToken {
                user_id: id,
                version: 7
            }
        );
    }

    #[test]
    fn issued_tokens_are_standard_jwt() {
        // Заголовок и структура - обычный JWT, который прочитает любая библиотека.
        let token = keys().issue(Uuid::new_v4(), 1).unwrap();
        let header = token.split('.').next().unwrap();

        assert_eq!(
            URL_SAFE_NO_PAD.decode(header).unwrap(),
            br#"{"typ":"JWT","alg":"HS256"}"#
        );
        assert_eq!(token.split('.').count(), 3);
    }

    #[test]
    fn rejects_token_signed_with_other_secret() {
        let theirs = JwtKeys::new(
            "another_secret_another_secret_0000",
            Duration::from_secs(60),
        );

        let token = theirs.issue(Uuid::new_v4(), 0).unwrap();
        assert!(keys().verify(&token).is_err());
    }

    #[test]
    fn rejects_tampered_and_garbage_tokens() {
        let keys = keys();
        let token = keys.issue(Uuid::new_v4(), 0).unwrap();

        let mut extended = token.clone();
        extended.push('x');
        assert!(keys.verify(&extended).is_err());

        // Подмена полезной нагрузки (другой пользователь / версия) при прежней подписи.
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged_payload = URL_SAFE_NO_PAD.encode(format!(
            r#"{{"sub":"{USER}","iat":0,"exp":4102444800,"ver":0}}"#
        ));
        parts[1] = &forged_payload;
        assert!(keys.verify(&parts.join(".")).is_err());

        for garbage in ["not.a.jwt", "", ".", "..", "a.b", "a.b.c.d", "a.b.c"] {
            assert!(keys.verify(garbage).is_err(), "{garbage:?}");
        }
    }

    #[test]
    fn rejects_other_algorithms_including_none() {
        assert!(keys().verify(ALG_NONE).is_err());

        // alg=none без подписи и с чужим заголовком не проходит ни в каком виде.
        let none_header = URL_SAFE_NO_PAD.encode(r#"{"alg":"none","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"{USER}","exp":4102444800}}"#));
        assert!(keys().verify(&format!("{none_header}.{payload}.")).is_err());

        // Заголовок HS512 с подписью HS256 отвергается: алгоритм не выбирается по токену.
        let hs512 = URL_SAFE_NO_PAD.encode(r#"{"alg":"HS512","typ":"JWT"}"#);
        let (_, rest) = WITH_VER_7.split_once('.').unwrap();
        assert!(keys().verify(&format!("{hs512}.{rest}")).is_err());
    }

    #[test]
    fn expiry_respects_leeway() {
        let keys = keys();
        let id = Uuid::new_v4();
        let claims = Claims {
            sub: id.to_string(),
            iat: 1000,
            exp: 2000,
            ver: 0,
        };
        let token = keys.sign(&claims).unwrap();

        assert!(keys.verify_at(&token, 2000).is_ok());
        assert!(keys.verify_at(&token, 2000 + LEEWAY_SECS).is_ok());
        assert!(keys.verify_at(&token, 2000 + LEEWAY_SECS + 1).is_err());
    }

    #[test]
    fn rejects_expired_token() {
        let keys = keys();
        let claims = Claims {
            sub: Uuid::new_v4().to_string(),
            iat: now_secs() - 1000,
            exp: now_secs() - 500,
            ver: 0,
        };
        let token = keys.sign(&claims).unwrap();

        assert!(keys.verify(&token).is_err());
    }

    #[test]
    fn rejects_non_uuid_subject() {
        let keys = keys();
        let token = keys
            .sign(&Claims {
                sub: "not-a-uuid".to_string(),
                iat: now_secs(),
                exp: now_secs() + 60,
                ver: 0,
            })
            .unwrap();

        assert!(keys.verify(&token).is_err());
    }
}
