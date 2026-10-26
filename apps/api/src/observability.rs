//! Логи и наблюдаемость запросов: request ID, JSON-логи, HTTP-метрики.

use std::time::Instant;

use axum::{
    extract::{MatchedPath, Request},
    http::{HeaderName, HeaderValue, Method},
    middleware::Next,
    response::Response,
};
use metrics::{counter, gauge, histogram};
use tracing::{Instrument, info_span};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use crate::{config::LogFormat, metrics::HTTP_DURATION};

pub const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");

/// Настраивает `tracing`. Уровень - через `RUST_LOG`, формат - `LOG_FORMAT=text|json`.
pub fn init_logging(format: LogFormat) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("rtchesz_api=info,tower_http=info"));

    let builder = tracing_subscriber::fmt().with_env_filter(filter);

    match format {
        LogFormat::Text => builder.init(),
        // В JSON-режиме поля спана (`request_id`, `method`, `path`) попадают в каждую строку
        // события, которое произошло во время запроса, в объект `span`.
        LogFormat::Json => builder
            .json()
            .with_current_span(true)
            .with_span_list(false)
            .init(),
    }
}

/// Чужой `X-Request-Id` принимаем, только если он короткий и безопасный для логов.
fn incoming_request_id(request: &Request) -> Option<String> {
    let value = request.headers().get(REQUEST_ID_HEADER)?.to_str().ok()?;
    let valid = (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');

    valid.then(|| value.to_string())
}

/// Метка метода для метрик: произвольные методы не должны плодить новые серии.
fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::PATCH => "PATCH",
        Method::DELETE => "DELETE",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        _ => "OTHER",
    }
}

/// Счётчик активных запросов, который уменьшается и при обрыве соединения клиентом.
struct InFlight;

impl InFlight {
    fn new() -> Self {
        gauge!("http_requests_in_flight").increment(1.0);
        Self
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        gauge!("http_requests_in_flight").decrement(1.0);
    }
}

/// Middleware: request ID + спан логов + метрики + одна строка лога на запрос.
pub async fn observe(mut request: Request, next: Next) -> Response {
    let request_id = incoming_request_id(&request).unwrap_or_else(|| Uuid::new_v4().to_string());
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        // Обработчики видят тот же ID в заголовке запроса.
        request.headers_mut().insert(REQUEST_ID_HEADER, value);
    }

    let method = request.method().clone();
    let path = request.uri().path().to_string();
    // Шаблон маршрута (`/auth/login`), а не реальный путь: иначе метрики разрастутся.
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_string())
        .unwrap_or_else(|| "unmatched".to_string());

    let span = info_span!("http", %request_id, %method, %path);
    let _in_flight = InFlight::new();
    let started = Instant::now();

    let mut response = next.run(request).instrument(span.clone()).await;

    let elapsed = started.elapsed();
    let status = response.status();

    counter!(
        "http_requests_total",
        "method" => method_label(&method),
        "path" => route.clone(),
        "status" => status.as_u16().to_string(),
    )
    .increment(1);
    histogram!(
        HTTP_DURATION,
        "method" => method_label(&method),
        "path" => route.clone(),
    )
    .record(elapsed.as_secs_f64());

    let latency_ms = u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX);
    span.in_scope(|| {
        if status.is_server_error() {
            tracing::error!(status = status.as_u16(), latency_ms, "request finished");
        } else if route == "/health" && status.is_success() {
            // Проверки живости идут часто (Docker, мониторинг) - не засоряем лог.
            tracing::debug!(status = status.as_u16(), latency_ms, "request finished");
        } else {
            tracing::info!(status = status.as_u16(), latency_ms, "request finished");
        }
    });

    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert(REQUEST_ID_HEADER, value);
    }

    response
}
