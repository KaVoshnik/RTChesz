//! Метрики в формате Prometheus.
//!
//! Рекордер глобальный и ставится один раз на процесс (`init`), поэтому `AppState::new`
//! можно вызывать много раз (тесты). Отдаются метрики отдельным сервером на `METRICS_ADDR`
//! (см. `routes::build_metrics_router`), а не через публичный порт API.

use std::{
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

use metrics::{describe_counter, describe_gauge, describe_histogram, gauge};
use metrics_exporter_prometheus::{Matcher, PrometheusBuilder, PrometheusHandle};
use sqlx::PgPool;

pub const HTTP_DURATION: &str = "http_request_duration_seconds";

static HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Ставит глобальный рекордер (один раз) и возвращает его handle.
pub fn init() -> &'static PrometheusHandle {
    HANDLE.get_or_init(|| {
        let handle = PrometheusBuilder::new()
            .set_buckets_for_metric(
                Matcher::Full(HTTP_DURATION.to_string()),
                &[
                    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
                ],
            )
            .expect("histogram buckets are valid")
            .install_recorder()
            .expect("metrics recorder is installed once per process");

        describe_counter!("http_requests_total", "Обработанные HTTP-запросы");
        describe_histogram!(HTTP_DURATION, "Время обработки HTTP-запроса, секунды");
        describe_gauge!("http_requests_in_flight", "Запросы, обрабатываемые сейчас");
        describe_gauge!("db_pool_connections", "Соединения пула БД по состояниям");
        describe_counter!("auth_logins_total", "Попытки входа по результату");
        describe_counter!("auth_registrations_total", "Успешные регистрации");
        describe_counter!("auth_logouts_total", "Выходы на всех устройствах");
        describe_counter!(
            "auth_password_resets_total",
            "Успешные смены пароля по коду"
        );
        describe_gauge!(
            "process_start_time_seconds",
            "Время запуска процесса (unix)"
        );
        describe_gauge!(
            "rtchesz_api_build_info",
            "Версия сборки (значение всегда 1)"
        );

        let started = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        gauge!("process_start_time_seconds").set(started);
        gauge!("rtchesz_api_build_info", "version" => env!("CARGO_PKG_VERSION")).set(1.0);

        handle
    })
}

/// Текст для `/metrics`. Состояние пула снимается в момент запроса.
pub fn render(pool: &PgPool, max_connections: u32) -> String {
    let size = pool.size();
    let idle = u32::try_from(pool.num_idle()).unwrap_or(u32::MAX);

    gauge!("db_pool_connections", "state" => "max").set(f64::from(max_connections));
    gauge!("db_pool_connections", "state" => "open").set(f64::from(size));
    gauge!("db_pool_connections", "state" => "idle").set(f64::from(idle));
    gauge!("db_pool_connections", "state" => "in_use").set(f64::from(size.saturating_sub(idle)));

    init().render()
}

/// Раз в несколько секунд нужно вызывать, чтобы рекордер освобождал память гистограмм.
pub fn run_upkeep() {
    init().run_upkeep();
}
