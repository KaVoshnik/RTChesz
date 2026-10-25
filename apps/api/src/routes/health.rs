use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Value, json};

use crate::state::AppState;

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<Value>) {
    match sqlx::query("SELECT 1").execute(&state.pool).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok", "db": "ok" }))),
        Err(error) => {
            tracing::error!(%error, "health check: database is unreachable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "degraded", "db": "down" })),
            )
        }
    }
}
