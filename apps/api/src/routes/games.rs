use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    auth::AuthUser,
    chess::Color,
    error::AppError,
    extract::ApiJson,
    games::service::{self, color_of, color_str, live_remaining, turn_of},
    repo::games::{ABORTED, ACTIVE, FINISHED, GameRow, WAITING},
    state::AppState,
};

const LIST_LIMIT: i64 = 50;

// ---------- Запросы ----------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateGameRequest {
    time_control: TimeControlRequest,
    /// `white`, `black` или `random` (по умолчанию).
    color: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimeControlRequest {
    initial_secs: i64,
    #[serde(default)]
    increment_secs: i64,
}

#[derive(Debug, Deserialize)]
pub struct MoveRequest {
    #[serde(rename = "move")]
    uci: String,
}

#[derive(Debug, Deserialize)]
pub struct MineQuery {
    status: Option<String>,
}

// ---------- Ответы ----------

#[derive(Debug, Serialize)]
struct PlayerResponse {
    id: Uuid,
    username: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimeControlResponse {
    initial_secs: i64,
    increment_secs: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClockResponse {
    white_ms: i64,
    black_ms: i64,
    /// Чьи часы тикают прямо сейчас (`null` - часы стоят).
    running: Option<&'static str>,
}

/// Краткое описание партии для списков.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSummary {
    id: Uuid,
    status: String,
    white: Option<PlayerResponse>,
    black: Option<PlayerResponse>,
    time_control: TimeControlResponse,
    ply: usize,
    result: Option<String>,
    termination: Option<String>,
    created_at: DateTime<Utc>,
}

/// Полное состояние партии.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameResponse {
    #[serde(flatten)]
    summary: GameSummary,
    ruleset: String,
    /// Ходы в UCI по порядку.
    moves: Vec<String>,
    fen: String,
    /// Чей ход (`null`, если партия не идёт).
    turn: Option<&'static str>,
    clock: ClockResponse,
    draw_offered_by: Option<&'static str>,
    /// Время сервера: по нему клиент подгоняет локальные часы.
    server_time: DateTime<Utc>,
    started_at: Option<DateTime<Utc>>,
    finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
pub struct GameList {
    games: Vec<GameSummary>,
}

fn player(id: Option<Uuid>, username: &Option<String>) -> Option<PlayerResponse> {
    match (id, username) {
        (Some(id), Some(username)) => Some(PlayerResponse {
            id,
            username: username.clone(),
        }),
        _ => None,
    }
}

fn summary(row: &GameRow) -> GameSummary {
    GameSummary {
        id: row.id,
        status: row.status.clone(),
        white: player(row.white_id, &row.white_username),
        black: player(row.black_id, &row.black_username),
        time_control: TimeControlResponse {
            initial_secs: row.initial_ms / 1000,
            increment_secs: row.increment_ms / 1000,
        },
        ply: row.moves.len(),
        result: row.result.clone(),
        termination: row.termination.clone(),
        created_at: row.created_at,
    }
}

fn full(row: GameRow) -> Result<GameResponse, AppError> {
    let game = service::rebuild(&row)?;
    let active = row.status == ACTIVE;
    let turn = turn_of(row.moves.len());
    let (white_ms, black_ms) = live_remaining(&row);

    let running = (active && row.turn_started_at.is_some()).then(|| color_str(turn));
    let draw_offered_by = row
        .draw_offered_by
        .and_then(|offerer| color_of(&row, offerer).map(color_str));

    Ok(GameResponse {
        summary: summary(&row),
        ruleset: row.ruleset.clone(),
        fen: game.fen(),
        turn: active.then(|| color_str(turn)),
        clock: ClockResponse {
            white_ms,
            black_ms,
            running,
        },
        draw_offered_by,
        server_time: row.now,
        started_at: row.started_at,
        finished_at: row.finished_at,
        moves: row.moves,
    })
}

fn parse_id(raw: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(raw).map_err(|_| AppError::NotFound("Партия не найдена".to_string()))
}

fn parse_color(raw: Option<&str>) -> Result<Option<Color>, AppError> {
    match raw.map(str::trim) {
        None | Some("" | "random") => Ok(None),
        Some("white") => Ok(Some(Color::White)),
        Some("black") => Ok(Some(Color::Black)),
        Some(_) => Err(AppError::bad_request(
            "Цвет должен быть white, black или random",
        )),
    }
}

// ---------- Обработчики ----------

/// POST /games
pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    ApiJson(body): ApiJson<CreateGameRequest>,
) -> Result<(StatusCode, Json<GameResponse>), AppError> {
    let color = parse_color(body.color.as_deref())?;
    let row = service::create(
        &state,
        auth.id,
        color,
        body.time_control.initial_secs,
        body.time_control.increment_secs,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(full(row)?)))
}

/// GET /games/lobby - открытые вызовы других игроков.
pub async fn lobby(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<GameList>, AppError> {
    let rows = crate::repo::games::list_waiting(&state.pool, auth.id, LIST_LIMIT).await?;

    Ok(Json(GameList {
        games: rows.iter().map(summary).collect(),
    }))
}

/// GET /games/mine?status=active|waiting|finished|aborted
pub async fn mine(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(query): Query<MineQuery>,
) -> Result<Json<GameList>, AppError> {
    let status = match query.status.as_deref() {
        None | Some("") => None,
        Some(status @ (WAITING | ACTIVE | FINISHED | ABORTED)) => Some(status),
        Some(_) => {
            return Err(AppError::bad_request(
                "status должен быть waiting, active, finished или aborted",
            ));
        }
    };

    let rows = crate::repo::games::list_for_user(&state.pool, auth.id, status, LIST_LIMIT).await?;

    Ok(Json(GameList {
        games: rows.iter().map(summary).collect(),
    }))
}

/// GET /games/{id}
pub async fn get(
    State(state): State<AppState>,
    _auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GameResponse>, AppError> {
    let row = service::get(&state, parse_id(&id)?).await?;

    Ok(Json(full(row)?))
}

/// POST /games/{id}/join
pub async fn join(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GameResponse>, AppError> {
    let row = service::join(&state, auth.id, parse_id(&id)?).await?;

    Ok(Json(full(row)?))
}

/// POST /games/{id}/moves
pub async fn play_move(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    ApiJson(body): ApiJson<MoveRequest>,
) -> Result<Json<GameResponse>, AppError> {
    let row = service::play_move(&state, auth.id, parse_id(&id)?, &body.uci).await?;

    Ok(Json(full(row)?))
}

/// POST /games/{id}/resign
pub async fn resign(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GameResponse>, AppError> {
    let row = service::resign(&state, auth.id, parse_id(&id)?).await?;

    Ok(Json(full(row)?))
}

/// POST /games/{id}/abort
pub async fn abort(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    match service::abort(&state, auth.id, parse_id(&id)?).await? {
        Some(row) => Ok(Json(serde_json::to_value(full(row)?).map_err(|error| {
            tracing::error!(%error, "failed to serialize game");
            AppError::Internal
        })?)),
        None => Ok(Json(serde_json::json!({ "message": "Вызов отменён" }))),
    }
}

/// POST /games/{id}/draw
pub async fn offer_draw(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GameResponse>, AppError> {
    let row = service::offer_draw(&state, auth.id, parse_id(&id)?).await?;

    Ok(Json(full(row)?))
}

/// POST /games/{id}/draw/decline
pub async fn decline_draw(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<GameResponse>, AppError> {
    let row = service::decline_draw(&state, auth.id, parse_id(&id)?).await?;

    Ok(Json(full(row)?))
}
