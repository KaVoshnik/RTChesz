mod auth;
mod health;

use axum::{
    Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderValue, Method, header},
    middleware,
    response::IntoResponse,
    routing::{get, post},
};
use tower_http::cors::CorsLayer;

use crate::{
    config::Config,
    error::AppError,
    observability::{self, REQUEST_ID_HEADER},
    state::AppState,
};

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
        .route("/auth/logout", post(auth::logout))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        // Request ID, логи запросов и метрики. Должен быть снаружи обработчиков и внутри CORS.
        .layer(middleware::from_fn(observability::observe))
        // CORS - самый внешний слой, чтобы заголовки получали и ошибки (404, 429...).
        .layer(cors)
        .with_state(state)
}

/// Отдельный роутер для `/metrics`. Слушает свой адрес (`METRICS_ADDR`), который не публикуется
/// наружу: Prometheus ходит к нему внутри docker-сети.
pub fn build_metrics_router(state: AppState) -> Router {
    Router::new()
        .route("/metrics", get(metrics))
        .with_state(state)
}

async fn metrics(State(state): State<AppState>) -> impl IntoResponse {
    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        crate::metrics::render(&state.pool, state.config.db_max_connections),
    )
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
        .expose_headers([header::RETRY_AFTER, REQUEST_ID_HEADER])
        .max_age(std::time::Duration::from_secs(3600))
}

async fn not_found() -> AppError {
    AppError::NotFound("Маршрут не найден".to_string())
}
