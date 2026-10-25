mod auth;
mod health;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, header},
    routing::{get, post},
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::{config::Config, error::AppError, state::AppState};

/// Максимальный размер тела запроса. Все наши JSON-запросы - считанные байты.
const MAX_BODY_BYTES: usize = 16 * 1024;

pub fn build_router(state: AppState) -> Router {
    let cors = cors_layer(&state.config);

    Router::new()
        .route("/health", get(health::health))
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/forgot-password", post(auth::forgot_password))
        .route("/auth/verify-code", post(auth::verify_code))
        .route("/auth/reset-password", post(auth::reset_password))
        .route("/auth/me", get(auth::me))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(TraceLayer::new_for_http())
        // CORS - самый внешний слой, чтобы заголовки получали и ошибки (404, 429...).
        .layer(cors)
        .with_state(state)
}

fn cors_layer(config: &Config) -> CorsLayer {
    let origins: Vec<HeaderValue> = config
        .cors_origins
        .iter()
        .filter_map(|origin| match origin.parse() {
            Ok(value) => Some(value),
            Err(_) => {
                tracing::warn!(%origin, "ignoring invalid CORS origin");
                None
            }
        })
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        // Без этого браузер не отдаст фронту заголовок Retry-After (см. api.ts).
        .expose_headers([header::RETRY_AFTER])
        .max_age(std::time::Duration::from_secs(3600))
}

async fn not_found() -> AppError {
    AppError::NotFound("Маршрут не найден".to_string())
}
