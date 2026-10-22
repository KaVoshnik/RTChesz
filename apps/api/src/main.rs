mod config;
mod db;

use axum::{routing::get, Json, Router};
use serde_json::json;
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

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await?;

    let app = Router::new().route("/health", get(health));

    
    let address: SocketAddr = format!("{}:{}", config.host, config.port)
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