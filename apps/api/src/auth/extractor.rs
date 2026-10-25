use axum::{extract::FromRequestParts, http::header::AUTHORIZATION, http::request::Parts};
use uuid::Uuid;

use crate::{error::AppError, state::AppState};

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

        let id = state.jwt.verify(token.trim())?;
        Ok(AuthUser { id })
    }
}
