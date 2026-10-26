use axum::{extract::FromRequestParts, http::header::AUTHORIZATION, http::request::Parts};
use uuid::Uuid;

use crate::{error::AppError, repo::users, state::AppState};

/// Авторизованный пользователь. Достаточно добавить аргумент `AuthUser`
/// в обработчик, чтобы закрыть маршрут токеном (`Authorization: Bearer <jwt>`).
#[derive(Debug, Clone, Copy)]
pub struct AuthUser {
    pub id: Uuid,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let required = || AppError::unauthorized("Требуется авторизация");

        let header = parts
            .headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(required)?;

        let (scheme, token) = header.split_once(' ').ok_or_else(required)?;
        if !scheme.eq_ignore_ascii_case("bearer") {
            return Err(required());
        }

        let verified = state.jwt.verify(token.trim())?;

        // Подпись верна, но токен мог быть отозван (смена пароля, выход) или
        // пользователь удалён: сверяем версию с БД.
        let current = users::token_version(&state.pool, verified.user_id).await?;
        if current != Some(verified.version) {
            return Err(AppError::unauthorized(
                "Недействительный или просроченный токен",
            ));
        }

        Ok(AuthUser {
            id: verified.user_id,
        })
    }
}
