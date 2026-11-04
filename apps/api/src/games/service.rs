//! Операции над партией. Каждая идёт в одной транзакции и держит блокировку строки партии
//! (`SELECT ... FOR UPDATE`), поэтому ходы, сдача и ничья одной партии выполняются строго по очереди.
//!
//! Время партии считает БД. Флажок и таймаут первого хода применяются лениво: любое обращение
//! к партии сначала проверяет, не вышло ли время, и фиксирует результат. Фоновая задача
//! ([`sweep`]) делает то же самое для партий, к которым давно никто не обращался.

use std::time::Duration;

use metrics::counter;
use sqlx::{PgConnection, Postgres, Transaction};
use uuid::Uuid;

use super::clock::{Clocks, validate_time_control};
use crate::{
    chess::{Color, Game, GameError, Move},
    error::AppError,
    repo::games::{self, ABORTED, ACTIVE, FINISHED, Finish, GameRow, WAITING},
    state::AppState,
};

/// Сколько открытых вызовов может висеть у одного игрока.
const MAX_OPEN_CHALLENGES: i64 = 5;
const CREATE_PER_USER: (u32, Duration) = (20, Duration::from_secs(60 * 60));
const MOVE_PER_USER: (u32, Duration) = (20, Duration::from_secs(10));
/// Предложения ничьей в одной партии от одного игрока.
const DRAW_OFFERS: (u32, Duration) = (5, Duration::from_secs(10 * 60));
/// Сколько партий за раз проверяет фоновая задача.
const SWEEP_BATCH: i64 = 100;

/// Чьи часы сейчас должны были остановить партию.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// Не сделан первый ход - партию отменяем.
    FirstMove,
    /// У стороны кончилось время.
    Flag(Color),
}

pub fn turn_of(plies: usize) -> Color {
    if plies.is_multiple_of(2) {
        Color::White
    } else {
        Color::Black
    }
}

pub fn color_str(color: Color) -> &'static str {
    match color {
        Color::White => "white",
        Color::Black => "black",
    }
}

pub fn color_of(row: &GameRow, user: Uuid) -> Option<Color> {
    if row.white_id == Some(user) {
        Some(Color::White)
    } else if row.black_id == Some(user) {
        Some(Color::Black)
    } else {
        None
    }
}

/// Часы партии, как они записаны в БД.
pub fn stored_clocks(row: &GameRow) -> Clocks {
    Clocks {
        white_ms: row.white_ms,
        black_ms: row.black_ms,
        running_since: row.turn_started_at,
    }
}

/// Остаток времени обеих сторон на момент `row.now` (для идущей партии часы тикают).
pub fn live_remaining(row: &GameRow) -> (i64, i64) {
    let clocks = stored_clocks(row);
    let turn = turn_of(row.moves.len());

    if row.status == ACTIVE {
        (
            clocks.remaining(Color::White, turn, row.now),
            clocks.remaining(Color::Black, turn, row.now),
        )
    } else {
        (row.white_ms, row.black_ms)
    }
}

/// Восстанавливает партию по ходам из БД.
pub fn rebuild(row: &GameRow) -> Result<Game, AppError> {
    let corrupted = |what: &str| {
        tracing::error!(game_id = %row.id, "stored game is broken: {what}");
        AppError::Internal
    };

    let mut moves = Vec::with_capacity(row.moves.len());
    for text in &row.moves {
        moves.push(Move::from_uci(text).map_err(|error| corrupted(&error.to_string()))?);
    }

    Game::from_moves(&row.ruleset, &moves).map_err(|error| corrupted(&error.to_string()))
}

/// Нужно ли остановить партию из-за времени (без записи в БД).
fn due(row: &GameRow, first_move_timeout: Duration) -> Option<Due> {
    if row.status != ACTIVE {
        return None;
    }

    let plies = row.moves.len();
    if plies < 2 {
        let since = row.last_move_at.unwrap_or(row.created_at);
        let waited = (row.now - since).to_std().unwrap_or_default();

        return (waited >= first_move_timeout).then_some(Due::FirstMove);
    }

    let turn = turn_of(plies);
    stored_clocks(row)
        .is_flagged(turn, row.now)
        .then_some(Due::Flag(turn))
}

fn ended_error(due: Due) -> AppError {
    match due {
        Due::Flag(_) => AppError::Conflict("Время вышло, партия окончена".to_string()),
        Due::FirstMove => {
            AppError::Conflict("Партия отменена: первый ход не сделан вовремя".to_string())
        }
    }
}

/// Закрывает партию: пишет результат, остановленные часы и считает метрику.
async fn end(
    conn: &mut PgConnection,
    row: &GameRow,
    status: &str,
    winner: Option<Option<Color>>,
    termination: &str,
    clocks: Clocks,
) -> Result<(), sqlx::Error> {
    let result = winner.map(|winner| winner.map_or("draw", color_str));

    let finish = Finish {
        status,
        result,
        termination,
        white_ms: clocks.white_ms,
        black_ms: clocks.black_ms,
    };
    games::finish(&mut *conn, row.id, &finish, row.now).await?;

    counter!("games_finished_total", "termination" => termination.to_string()).increment(1);

    Ok(())
}

/// Фиксирует флажок или таймаут первого хода, если время вышло. `Some` - партия закрыта.
async fn apply_timeouts(
    conn: &mut PgConnection,
    row: &GameRow,
    first_move_timeout: Duration,
) -> Result<Option<Due>, sqlx::Error> {
    let Some(reason) = due(row, first_move_timeout) else {
        return Ok(None);
    };

    match reason {
        Due::FirstMove => {
            let (white_ms, black_ms) = (row.white_ms, row.black_ms);
            let clocks = Clocks {
                white_ms,
                black_ms,
                running_since: None,
            };
            end(conn, row, ABORTED, None, "aborted", clocks).await?;
        }
        Due::Flag(loser) => {
            let clocks = stored_clocks(row).flagged(loser);
            end(
                conn,
                row,
                FINISHED,
                Some(Some(loser.opposite())),
                "timeout",
                clocks,
            )
            .await?;
        }
    }

    Ok(Some(reason))
}

/// Часы на момент окончания партии по решению игроков (сдача, ничья, отмена).
fn clocks_now(row: &GameRow) -> Clocks {
    let (white_ms, black_ms) = live_remaining(row);

    Clocks {
        white_ms,
        black_ms,
        running_since: None,
    }
}

async fn lock(conn: &mut PgConnection, id: Uuid) -> Result<GameRow, AppError> {
    games::fetch_for_update(conn, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Партия не найдена".to_string()))
}

/// Начало действия игрока: транзакция, блокировка, проверка участия и времени.
/// Если время уже вышло, результат фиксируется, а игрок получает `409`.
async fn begin_action(
    state: &AppState,
    id: Uuid,
    user: Uuid,
) -> Result<(Transaction<'static, Postgres>, GameRow, Color), AppError> {
    let mut tx = state.pool.begin().await?;
    let row = lock(&mut tx, id).await?;

    let color = color_of(&row, user)
        .ok_or_else(|| AppError::Forbidden("Вы не участник этой партии".to_string()))?;

    if let Some(reason) =
        apply_timeouts(&mut tx, &row, state.config.game_first_move_timeout).await?
    {
        tx.commit().await?;
        return Err(ended_error(reason));
    }

    Ok((tx, row, color))
}

fn require_active(row: &GameRow) -> Result<(), AppError> {
    match row.status.as_str() {
        ACTIVE => Ok(()),
        WAITING => Err(AppError::Conflict("Ждём соперника".to_string())),
        _ => Err(AppError::Conflict("Партия уже окончена".to_string())),
    }
}

// ---------- Публичные операции ----------

/// Какой цвет хочет создатель вызова. `None` - случайный.
pub async fn create(
    state: &AppState,
    user: Uuid,
    color: Option<Color>,
    initial_secs: i64,
    increment_secs: i64,
) -> Result<GameRow, AppError> {
    state
        .limiter
        .hit(
            &format!("game:create:{user}"),
            CREATE_PER_USER.0,
            CREATE_PER_USER.1,
        )
        .map_err(AppError::TooManyRequests)?;

    let (initial_ms, increment_ms) = validate_time_control(initial_secs, increment_secs)?;

    // Проверка не атомарна: при гонке вызовов может стать на один-два больше. Для лимита это нормально.
    if games::count_waiting_by(&state.pool, user).await? >= MAX_OPEN_CHALLENGES {
        return Err(AppError::Conflict(
            "У вас слишком много открытых вызовов: дождитесь соперника или отмените лишние"
                .to_string(),
        ));
    }

    let creator_is_white = match color {
        Some(color) => color == Color::White,
        None => {
            getrandom::u32().map_err(|error| {
                tracing::error!(%error, "OS random generator failed");
                AppError::Internal
            })? & 1
                == 0
        }
    };

    let id = games::insert_waiting(
        &state.pool,
        user,
        creator_is_white,
        initial_ms,
        increment_ms,
    )
    .await?;
    counter!("games_created_total").increment(1);

    get(state, id).await
}

/// Принять вызов: занимаем свободный цвет, партия начинается.
pub async fn join(state: &AppState, user: Uuid, id: Uuid) -> Result<GameRow, AppError> {
    let mut tx = state.pool.begin().await?;
    let row = lock(&mut tx, id).await?;

    if row.status != WAITING {
        return Err(AppError::Conflict(
            "К этой партии уже нельзя присоединиться".to_string(),
        ));
    }
    if row.creator_id == user {
        return Err(AppError::bad_request("Нельзя играть против самого себя"));
    }

    games::start(&mut *tx, id, user, row.now).await?;
    tx.commit().await?;
    counter!("games_started_total").increment(1);

    get(state, id).await
}

/// Ход в записи UCI (`e2e4`, `e7e8q`).
pub async fn play_move(
    state: &AppState,
    user: Uuid,
    id: Uuid,
    text: &str,
) -> Result<GameRow, AppError> {
    state
        .limiter
        .hit(
            &format!("move:user:{user}"),
            MOVE_PER_USER.0,
            MOVE_PER_USER.1,
        )
        .map_err(AppError::TooManyRequests)?;

    let mv = Move::from_uci(text.trim())
        .map_err(|_| AppError::bad_request("Некорректная запись хода, пример: e2e4 или e7e8q"))?;

    let (mut tx, row, color) = begin_action(state, id, user).await?;
    require_active(&row)?;

    let plies = row.moves.len();
    if turn_of(plies) != color {
        return Err(AppError::Conflict("Сейчас не ваш ход".to_string()));
    }

    let mut game = rebuild(&row)?;
    let outcome = game.play(mv).map_err(|error| match error {
        GameError::IllegalMove(_) => AppError::bad_request("Недопустимый ход"),
        GameError::GameOver => AppError::Conflict("Партия уже окончена".to_string()),
        other => {
            tracing::error!(game_id = %id, %other, "unexpected error while playing a move");
            AppError::Internal
        }
    })?;

    // Время мы уже проверили в `begin_action`, поэтому `None` здесь невозможен.
    let clocks = stored_clocks(&row)
        .after_move(color, plies, row.increment_ms, row.now)
        .ok_or(AppError::Internal)?;

    games::save_move(
        &mut *tx,
        id,
        &mv.to_string(),
        clocks.white_ms,
        clocks.black_ms,
        clocks.running_since,
        row.now,
    )
    .await?;
    counter!("game_moves_total").increment(1);

    if let Some(outcome) = outcome {
        let stopped = Clocks {
            running_since: None,
            ..clocks
        };
        end(
            &mut tx,
            &row,
            FINISHED,
            Some(outcome.winner),
            outcome.termination.as_str(),
            stopped,
        )
        .await?;
    }

    tx.commit().await?;

    get(state, id).await
}

pub async fn resign(state: &AppState, user: Uuid, id: Uuid) -> Result<GameRow, AppError> {
    let (mut tx, row, color) = begin_action(state, id, user).await?;
    require_active(&row)?;

    end(
        &mut tx,
        &row,
        FINISHED,
        Some(Some(color.opposite())),
        "resignation",
        clocks_now(&row),
    )
    .await?;
    tx.commit().await?;

    get(state, id).await
}

/// Отмена: свой вызов (удаляется, `None`) или партию, в которой ещё не сделано по первому ходу.
pub async fn abort(state: &AppState, user: Uuid, id: Uuid) -> Result<Option<GameRow>, AppError> {
    let (mut tx, row, _) = begin_action(state, id, user).await?;

    match row.status.as_str() {
        WAITING => {
            games::delete_waiting(&mut *tx, id).await?;
            tx.commit().await?;

            Ok(None)
        }
        ACTIVE if row.moves.len() < 2 => {
            end(&mut tx, &row, ABORTED, None, "aborted", clocks_now(&row)).await?;
            tx.commit().await?;

            Ok(Some(get(state, id).await?))
        }
        ACTIVE => Err(AppError::Conflict(
            "Партию уже нельзя отменить, можно сдаться".to_string(),
        )),
        _ => Err(AppError::Conflict("Партия уже окончена".to_string())),
    }
}

/// Предложить ничью. Если соперник уже предлагал - это согласие, партия заканчивается вничью.
pub async fn offer_draw(state: &AppState, user: Uuid, id: Uuid) -> Result<GameRow, AppError> {
    let (mut tx, row, _) = begin_action(state, id, user).await?;
    require_active(&row)?;

    match row.draw_offered_by {
        Some(offerer) if offerer != user => {
            end(
                &mut tx,
                &row,
                FINISHED,
                Some(None),
                "draw_agreement",
                clocks_now(&row),
            )
            .await?;
        }
        Some(_) => {
            return Err(AppError::Conflict("Вы уже предложили ничью".to_string()));
        }
        None => {
            if row.moves.len() < 2 {
                return Err(AppError::Conflict(
                    "Ничью можно предложить после первых ходов".to_string(),
                ));
            }

            state
                .limiter
                .hit(&format!("draw:{id}:{user}"), DRAW_OFFERS.0, DRAW_OFFERS.1)
                .map_err(AppError::TooManyRequests)?;

            games::set_draw_offer(&mut *tx, id, Some(user)).await?;
        }
    }

    tx.commit().await?;

    get(state, id).await
}

/// Отклонить предложение ничьей (или забрать своё).
pub async fn decline_draw(state: &AppState, user: Uuid, id: Uuid) -> Result<GameRow, AppError> {
    let (mut tx, row, _) = begin_action(state, id, user).await?;
    require_active(&row)?;

    if row.draw_offered_by.is_none() {
        return Err(AppError::Conflict("Нет предложения ничьей".to_string()));
    }

    games::set_draw_offer(&mut *tx, id, None).await?;
    tx.commit().await?;

    get(state, id).await
}

/// Партия в актуальном состоянии: если время вышло, результат фиксируется перед ответом.
pub async fn get(state: &AppState, id: Uuid) -> Result<GameRow, AppError> {
    let row = games::fetch(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Партия не найдена".to_string()))?;

    if due(&row, state.config.game_first_move_timeout).is_none() {
        return Ok(row);
    }

    settle(state, id).await?;

    games::fetch(&state.pool, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Партия не найдена".to_string()))
}

/// Закрывает партию, если время вышло (под блокировкой: гонки с ходом исключены).
async fn settle(state: &AppState, id: Uuid) -> Result<(), AppError> {
    let mut tx = state.pool.begin().await?;

    if let Some(row) = games::fetch_for_update(&mut tx, id).await? {
        apply_timeouts(&mut tx, &row, state.config.game_first_move_timeout).await?;
    }

    tx.commit().await?;

    Ok(())
}

/// Фоновая проверка: закрывает партии, где вышло время, даже если в них никто не заходит.
pub async fn sweep(state: &AppState) {
    let ids = match games::due_for_check(
        &state.pool,
        state.config.game_first_move_timeout,
        SWEEP_BATCH,
    )
    .await
    {
        Ok(ids) => ids,
        Err(error) => {
            tracing::warn!(%error, "failed to look for games with expired time");
            return;
        }
    };

    for id in ids {
        if let Err(error) = settle(state, id).await {
            tracing::warn!(game_id = %id, %error, "failed to settle a game");
        }
    }
}

/// Удаляет вызовы, на которые никто не откликнулся.
pub async fn purge_stale_challenges(state: &AppState) {
    match games::purge_stale_waiting(&state.pool, state.config.game_challenge_ttl).await {
        Ok(0) => {}
        Ok(count) => tracing::info!(count, "removed stale game challenges"),
        Err(error) => tracing::warn!(%error, "failed to remove stale game challenges"),
    }
}
