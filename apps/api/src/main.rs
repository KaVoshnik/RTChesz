mod config;
mod db;
mod error;
mod state;

use axum::{Json, Router, routing::get};
use serde_json::json;
use state::AppState;
use std::net::SocketAddr;

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok"
    }))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let config = config::Config::from_env()?;

    let pool = db::connect(&config.database_url).await?;

    sqlx::migrate!("./migrations").run(&pool).await?;

    let state = AppState {
        pool,
        config: config.clone(),
    };

    let app = Router::new()
        .route("/health", get(health))
        .with_state(state);

    let address: SocketAddr =
        format!("{}:{}", config.host, config.port)
            .parse()
            .map_err(|error| {
                eprintln!(
                    "Invalid server address '{}:{}': {}",
                    config.host, config.port, error
                );
                error
            })?;

    let listener = tokio::net::TcpListener::bind(address).await?;

    println!("API listening on {address}");

    axum::serve(listener, app).await?;

    Ok(())
}
