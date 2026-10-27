use std::sync::Arc;

use sqlx::PgPool;

use crate::{
    auth::{JwtKeys, password},
    config::Config,
    error::AppError,
    mail::Mailer,
    rate_limit::RateLimiter,
};

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: Arc<Config>,
    pub jwt: Arc<JwtKeys>,
    pub limiter: Arc<RateLimiter>,
    pub mailer: Arc<Mailer>,
    /// Хэш-пустышка: при входе с несуществующим email проверяем пароль по нему,
    /// чтобы время ответа не выдавало, зарегистрирован ли адрес.
    pub dummy_hash: Arc<String>,
}

impl AppState {
    pub fn new(pool: PgPool, config: Config, mailer: Mailer) -> Result<Self, AppError> {
        // Рекордер метрик ставится один раз на процесс; повторные вызовы безвредны.
        crate::metrics::init();

        let jwt = JwtKeys::new(&config.jwt_secret, config.jwt_ttl);

        Ok(Self {
            pool,
            jwt: Arc::new(jwt),
            limiter: Arc::new(RateLimiter::new()),
            mailer: Arc::new(mailer),
            dummy_hash: Arc::new(password::hash("rtchesz-dummy-password")?),
            config: Arc::new(config),
        })
    }
}
