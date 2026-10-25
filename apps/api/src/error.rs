use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::json;

/// Ошибки API.
///
/// Формат ответа: `{ "code": "bad_request", "message": "Текст для пользователя" }`.
/// Поле `message` на верхнем уровне читает фронтенд (`data.message` в `api.ts`),
/// поэтому тексты написаны на русском и безопасны для показа пользователю.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    /// Слишком много запросов; число - через сколько секунд можно повторить.
    #[error("Слишком много попыток")]
    TooManyRequests(u64),

    #[error("Внутренняя ошибка сервера")]
    Internal,
}

impl AppError {
    pub fn bad_request(message: impl Into<String>) -> Self {
        AppError::BadRequest(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        AppError::Unauthorized(message.into())
    }

    fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code(&self) -> &'static str {
        match self {
            AppError::BadRequest(_) => "bad_request",
            AppError::Unauthorized(_) => "unauthorized",
            AppError::NotFound(_) => "not_found",
            AppError::Conflict(_) => "conflict",
            AppError::TooManyRequests(_) => "too_many_requests",
            AppError::Internal => "internal",
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = Json(json!({
            "code": self.code(),
            "message": self.to_string(),
        }));

        let mut response = (self.status(), body).into_response();

        match &self {
            AppError::TooManyRequests(retry_after) => {
                if let Ok(value) = HeaderValue::from_str(&retry_after.to_string()) {
                    response.headers_mut().insert(header::RETRY_AFTER, value);
                }
            }
            AppError::Unauthorized(_) => {
                response
                    .headers_mut()
                    .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
            }
            _ => {}
        }

        response
    }
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database error");
        AppError::Internal
    }
}
