use std::{
    future::IntoFuture,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    time::Duration,
};

use rtchesz_api::{
    config::Config, db, mail::Mailer, metrics, observability, repo::password_resets, routes,
    state::AppState,
};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    // `rtchesz-api healthcheck` - проверка для Docker HEALTHCHECK (в образе нет curl).
    if std::env::args().nth(1).as_deref() == Some("healthcheck") {
        std::process::exit(healthcheck());
    }

    let config = Config::from_env()?;
    observability::init_logging(config.log_format);

    // Миграции идут отдельным соединением без statement_timeout.
    db::migrate(&config.database_url).await?;
    let pool = db::connect(&config).await?;

    let mailer = Mailer::from_config(config.smtp.as_ref())?;
    let address = (config.host.clone(), config.port);
    let metrics_addr = config.metrics_addr.clone();
    let shutdown_grace = config.shutdown_grace;
    let state = AppState::new(pool.clone(), config, mailer)?;

    let (stop_tx, stop_rx) = watch::channel(false);

    spawn_maintenance(state.clone());

    if let Some(addr) = metrics_addr {
        spawn_metrics_server(state.clone(), addr, stop_rx.clone()).await?;
    }

    let app = routes::build_router(state);

    let listener = tokio::net::TcpListener::bind(address).await?;
    tracing::info!("API listening on {}", listener.local_addr()?);

    let server = axum::serve(
        listener,
        // ConnectInfo нужен, чтобы знать IP клиента для лимитов запросов.
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(stopped(stop_rx))
    .into_future();
    tokio::pin!(server);

    tokio::select! {
        result = &mut server => result?,
        () = shutdown_signal() => {
            tracing::info!(
                grace_secs = shutdown_grace.as_secs(),
                "shutdown signal received: no new connections, waiting for active requests"
            );
            // Говорим серверам остановиться и даём активным запросам время дойти до конца.
            let _ = stop_tx.send(true);

            match tokio::time::timeout(shutdown_grace, &mut server).await {
                Ok(result) => result?,
                Err(_) => tracing::warn!("grace period is over: dropping remaining connections"),
            }
        }
    }

    pool.close().await;
    tracing::info!("bye");

    Ok(())
}

/// Фоновая уборка: старые записи восстановления пароля, истёкшие счётчики лимитов, метрики.
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

    tokio::spawn(async {
        let mut interval = tokio::time::interval(Duration::from_secs(10));

        loop {
            interval.tick().await;
            metrics::run_upkeep();
        }
    });
}

/// Отдельный сервер с `/metrics` на закрытом от интернета адресе.
async fn spawn_metrics_server(
    state: AppState,
    addr: String,
    stop: watch::Receiver<bool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("metrics listening on {}", listener.local_addr()?);

    let app = routes::build_metrics_router(state);

    tokio::spawn(async move {
        if let Err(error) = axum::serve(listener, app)
            .with_graceful_shutdown(stopped(stop))
            .await
        {
            tracing::error!(%error, "metrics server failed");
        }
    });

    Ok(())
}

/// Ждёт сигнала остановки.
async fn stopped(mut stop: watch::Receiver<bool>) {
    let _ = stop.wait_for(|stopped| *stopped).await;
}

/// SIGTERM (его шлёт `docker stop` / `docker compose up` при обновлении) и Ctrl-C.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "failed to listen for ctrl-c");
            std::future::pending::<()>().await;
        }
    };

    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut stream) => {
                stream.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to listen for SIGTERM");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
}

/// Код выхода 0 - API отвечает 200 на `/health` (то есть и БД доступна), иначе 1.
fn healthcheck() -> i32 {
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(3000);
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let timeout = Duration::from_secs(3);

    let result = (|| -> std::io::Result<String> {
        let mut stream = TcpStream::connect_timeout(&address, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(b"GET /health HTTP/1.0\r\nHost: localhost\r\n\r\n")?;

        let mut response = String::new();
        stream.read_to_string(&mut response)?;
        Ok(response)
    })();

    match result {
        Ok(response)
            if response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200") =>
        {
            0
        }
        _ => 1,
    }
}
