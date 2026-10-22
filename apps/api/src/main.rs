use axum::{routing::get, Json, Router};
use serde_json::json;
use tokio::net::TcpListener;

async fn health() -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok"
    }))
}

#[tokio::main]
async fn main() {
    let app = Router::new().route("/health", get(health)); // health route

    let listener = TcpListener::bind("127.0.0.1:3000").await.expect("Failed to bind TCP listener");

    println!("Api on http://127.0.0.1:3000");

    axum::serve(listener, app)
        .await
        .expect("Failed to start server");
}