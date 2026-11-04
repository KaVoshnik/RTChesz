//! Доступ к таблице `games`. Только SQL и учёт состояния: правила игры и часы - в `crate::games`.
//!
//! Время (`now`) во всех записях передаёт вызывающий код, считанное в той же транзакции
//! из БД (`clock_timestamp()`), - так часы не зависят от часов сервера API.

use std::time::Duration;

use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgExecutor};
use uuid::Uuid;

pub const WAITING: &str = "waiting";
pub const ACTIVE: &str = "active";
pub const FINISHED: &str = "finished";
pub const ABORTED: &str = "aborted";

/// Строка партии вместе с именами игроков и текущим временем БД (`now`).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct GameRow {
    pub id: Uuid,
    pub ruleset: String,
    pub status: String,
    pub creator_id: Uuid,
    pub white_id: Option<Uuid>,
    pub black_id: Option<Uuid>,
    pub white_username: Option<String>,
    pub black_username: Option<String>,
    pub initial_ms: i64,
    pub increment_ms: i64,
    pub white_ms: i64,
    pub black_ms: i64,
    pub turn_started_at: Option<DateTime<Utc>>,
    pub moves: Vec<String>,
    pub result: Option<String>,
    pub termination: Option<String>,
    pub draw_offered_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub last_move_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub now: DateTime<Utc>,
}

/// Общая часть всех выборок партии. Макрос, а не `format!`: sqlx принимает только литералы.
macro_rules! select_game {
    ($tail:literal) => {
        concat!(
            "SELECT g.id, g.ruleset, g.status, g.creator_id, g.white_id, g.black_id, \
                    wu.username AS white_username, bu.username AS black_username, \
                    g.initial_ms, g.increment_ms, g.white_ms, g.black_ms, g.turn_started_at, \
                    g.moves, g.result, g.termination, g.draw_offered_by, \
                    g.created_at, g.started_at, g.last_move_at, g.finished_at, \
                    clock_timestamp() AS now \
             FROM games g \
             LEFT JOIN users wu ON wu.id = g.white_id \
             LEFT JOIN users bu ON bu.id = g.black_id ",
            $tail
        )
    };
}

pub async fn fetch(
    executor: impl PgExecutor<'_>,
    id: Uuid,
) -> Result<Option<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>(select_game!("WHERE g.id = $1"))
        .bind(id)
        .fetch_optional(executor)
        .await
}

/// Читает партию и блокирует строку до конца транзакции: ходы одной партии идут строго по очереди.
pub async fn fetch_for_update(
    connection: &mut PgConnection,
    id: Uuid,
) -> Result<Option<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>(select_game!("WHERE g.id = $1 FOR UPDATE OF g"))
        .bind(id)
        .fetch_optional(connection)
        .await
}

pub async fn insert_waiting(
    executor: impl PgExecutor<'_>,
    creator_id: Uuid,
    creator_is_white: bool,
    initial_ms: i64,
    increment_ms: i64,
) -> Result<Uuid, sqlx::Error> {
    let (white, black) = if creator_is_white {
        (Some(creator_id), None)
    } else {
        (None, Some(creator_id))
    };

    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO games (creator_id, white_id, black_id, initial_ms, increment_ms, white_ms, black_ms) \
         VALUES ($1, $2, $3, $4, $5, $4, $4) RETURNING id",
    )
    .bind(creator_id)
    .bind(white)
    .bind(black)
    .bind(initial_ms)
    .bind(increment_ms)
    .fetch_one(executor)
    .await
}

pub async fn count_waiting_by(
    executor: impl PgExecutor<'_>,
    creator_id: Uuid,
) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM games WHERE creator_id = $1 AND status = 'waiting'",
    )
    .bind(creator_id)
    .fetch_one(executor)
    .await
}

/// Второй игрок занимает свободный цвет, партия начинается.
pub async fn start(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    joiner_id: Uuid,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE games SET \
             white_id = COALESCE(white_id, $2), \
             black_id = COALESCE(black_id, $2), \
             status = 'active', started_at = $3, last_move_at = $3 \
         WHERE id = $1 AND status = 'waiting'",
    )
    .bind(id)
    .bind(joiner_id)
    .bind(now)
    .execute(executor)
    .await?;

    Ok(result.rows_affected() == 1)
}

/// Дописывает ход и обновляет часы. Предложение ничьей после хода снимается.
pub async fn save_move(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    uci: &str,
    white_ms: i64,
    black_ms: i64,
    turn_started_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE games SET moves = array_append(moves, $2), white_ms = $3, black_ms = $4, \
             turn_started_at = $5, last_move_at = $6, draw_offered_by = NULL \
         WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .bind(uci)
    .bind(white_ms)
    .bind(black_ms)
    .bind(turn_started_at)
    .bind(now)
    .execute(executor)
    .await?;

    Ok(())
}

/// Как закончилась партия.
pub struct Finish<'a> {
    /// `finished` (с `result`) или `aborted` (без результата).
    pub status: &'a str,
    pub result: Option<&'a str>,
    pub termination: &'a str,
    pub white_ms: i64,
    pub black_ms: i64,
}

pub async fn finish(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    finish: &Finish<'_>,
    now: DateTime<Utc>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE games SET status = $2, result = $3, termination = $4, \
             white_ms = $5, black_ms = $6, turn_started_at = NULL, \
             draw_offered_by = NULL, finished_at = $7 \
         WHERE id = $1 AND status = 'active'",
    )
    .bind(id)
    .bind(finish.status)
    .bind(finish.result)
    .bind(finish.termination)
    .bind(finish.white_ms)
    .bind(finish.black_ms)
    .bind(now)
    .execute(executor)
    .await?;

    Ok(())
}

pub async fn set_draw_offer(
    executor: impl PgExecutor<'_>,
    id: Uuid,
    offered_by: Option<Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE games SET draw_offered_by = $2 WHERE id = $1 AND status = 'active'")
        .bind(id)
        .bind(offered_by)
        .execute(executor)
        .await?;

    Ok(())
}

/// Создатель отменяет свой вызов, пока никто не присоединился.
pub async fn delete_waiting(executor: impl PgExecutor<'_>, id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM games WHERE id = $1 AND status = 'waiting'")
        .bind(id)
        .execute(executor)
        .await?;

    Ok(())
}

/// Открытые вызовы других игроков, свежие первыми.
pub async fn list_waiting(
    executor: impl PgExecutor<'_>,
    exclude_user: Uuid,
    limit: i64,
) -> Result<Vec<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>(select_game!(
        "WHERE g.status = 'waiting' AND g.creator_id <> $1 \
         ORDER BY g.created_at DESC LIMIT $2"
    ))
    .bind(exclude_user)
    .bind(limit)
    .fetch_all(executor)
    .await
}

/// Партии пользователя (в том числе его собственные вызовы), свежие первыми.
pub async fn list_for_user(
    executor: impl PgExecutor<'_>,
    user_id: Uuid,
    status: Option<&str>,
    limit: i64,
) -> Result<Vec<GameRow>, sqlx::Error> {
    sqlx::query_as::<_, GameRow>(select_game!(
        "WHERE (g.white_id = $1 OR g.black_id = $1) \
           AND ($2::text IS NULL OR g.status = $2) \
         ORDER BY g.created_at DESC LIMIT $3"
    ))
    .bind(user_id)
    .bind(status)
    .bind(limit)
    .fetch_all(executor)
    .await
}

/// Идущие партии, у которых мог кончиться запас времени или время на первый ход.
/// Точное решение принимает `games::service` под блокировкой строки.
pub async fn due_for_check(
    executor: impl PgExecutor<'_>,
    first_move_timeout: Duration,
    limit: i64,
) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM games WHERE status = 'active' AND ( \
             (turn_started_at IS NOT NULL AND turn_started_at + \
                 (CASE WHEN cardinality(moves) % 2 = 0 THEN white_ms ELSE black_ms END) \
                 * INTERVAL '1 millisecond' <= clock_timestamp()) \
             OR (cardinality(moves) < 2 AND last_move_at + make_interval(secs => $1) <= clock_timestamp()) \
         ) LIMIT $2",
    )
    .bind(first_move_timeout.as_secs_f64())
    .bind(limit)
    .fetch_all(executor)
    .await
}

/// Удаляет вызовы, на которые никто не откликнулся.
pub async fn purge_stale_waiting(
    executor: impl PgExecutor<'_>,
    ttl: Duration,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM games WHERE status = 'waiting' \
         AND created_at < NOW() - make_interval(secs => $1)",
    )
    .bind(ttl.as_secs_f64())
    .execute(executor)
    .await?;

    Ok(result.rows_affected())
}
