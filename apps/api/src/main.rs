use std::time::Duration;

use rtchesz_api::{
    config::Config, db, mail::Mailer, repo::password_resets, routes, state::AppState,
};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("rtchesz_api=info,tower_http=info")),
        )
        .init();

    let config = Config::from_env()?;

    let pool = db::connect(&config.database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;

    let mailer = Mailer::from_config(config.smtp.as_ref())?;
    let address = (config.host.clone(), config.port);
    let state = AppState::new(pool, config, mailer)?;

    spawn_maintenance(state.clone());

    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("API listening on {}", listener.local_addr()?);

    axum::serve(
        listener,
        // ConnectInfo нужен, чтобы знать IP клиента для лимитов запросов.
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;

    Ok(())
}

/// Фоновая уборка: старые записи восстановления пароля и истёкшие счётчики лимитов.
fn spawn_maintenance(state: AppState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(10 * 60));

        loop {
            interval.tick().await;

            state.limiter.purge_expired();

            if let Err(error) = password_resets::purge_old(&state.pool).await {
                tracing::warn!(%error, "failed to purge old password resets");
            }
        }
    });
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        tracing::error!(%error, "failed to listen for ctrl-c");
    }

    tracing::info!("shutting down");
}
