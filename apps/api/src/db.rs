use std::time::Duration;

use sqlx::{
    PgConnection, PgPool,
    postgres::{PgConnectOptions, PgPoolOptions},
};

use crate::config::Config;

/// Пул соединений с жёсткими таймаутами: зависшая БД не должна вешать API навсегда.
///
/// * `acquire_timeout` - ожидание свободного соединения (иначе `sqlx::Error::PoolTimedOut`,
///   который API превращает в 503);
/// * `statement_timeout` - предел времени одного запроса на стороне Postgres;
/// * `idle_timeout`/`max_lifetime` - соединения периодически пересоздаются
///   (переживают рестарт БД и обрывы на NAT).
pub async fn connect(config: &Config) -> Result<PgPool, sqlx::Error> {
    connect_with(config.database_url.parse()?, config).await
}

/// То же, но с готовыми параметрами подключения (нужно тестам).
pub async fn connect_with(
    mut options: PgConnectOptions,
    config: &Config,
) -> Result<PgPool, sqlx::Error> {
    if !config.db_statement_timeout.is_zero() {
        options = options.options([(
            "statement_timeout",
            config.db_statement_timeout.as_millis().to_string(),
        )]);
    }

    PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .acquire_timeout(config.db_acquire_timeout)
        .idle_timeout(Duration::from_secs(10 * 60))
        .max_lifetime(Duration::from_secs(30 * 60))
        .connect_with(options)
        .await
}

/// Применяет миграции отдельным соединением БЕЗ `statement_timeout`:
/// долгая миграция (индекс на большой таблице) не должна обрываться по таймауту запросов.
pub async fn migrate(database_url: &str) -> Result<(), Box<dyn std::error::Error>> {
    use sqlx::Connection;

    let mut connection: PgConnection = PgConnection::connect(database_url).await?;
    sqlx::migrate!("./migrations").run(&mut connection).await?;
    connection.close().await?;

    Ok(())
}
