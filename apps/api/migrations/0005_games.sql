-- Партии. Источник истины - ruleset + список ходов в UCI (`moves`): позицию всегда можно
-- восстановить, проиграв ходы (chess::Game::from_moves).
--
-- Жизненный цикл `status`:
--   waiting  - открытый вызов, создатель занял один цвет, второго игрока нет;
--   active   - играем;
--   finished - есть результат (`result` + `termination`);
--   aborted  - отменена до начала игры по-настоящему (меньше двух ходов).
--
-- Часы. `white_ms`/`black_ms` - остаток времени каждой стороны на момент
-- `turn_started_at`. Тикает только часы стороны, чей сейчас ход, и только когда оба
-- игрока уже сделали по первому ходу (до этого `turn_started_at` = NULL). Текущий остаток
-- считается как `*_ms - (NOW() - turn_started_at)`, поэтому БД ничего не обновляет
-- каждую секунду. Все времена берутся по часам БД.
CREATE TABLE games (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),

    ruleset TEXT NOT NULL DEFAULT 'standard',
    status TEXT NOT NULL DEFAULT 'waiting',

    creator_id UUID NOT NULL REFERENCES users (id),
    white_id UUID REFERENCES users (id),
    black_id UUID REFERENCES users (id),

    initial_ms BIGINT NOT NULL,
    increment_ms BIGINT NOT NULL,
    white_ms BIGINT NOT NULL,
    black_ms BIGINT NOT NULL,
    turn_started_at TIMESTAMPTZ,

    moves TEXT[] NOT NULL DEFAULT '{}',

    result TEXT,
    termination TEXT,
    -- Кто предложил ничью (NULL - предложения нет).
    draw_offered_by UUID REFERENCES users (id),

    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    started_at TIMESTAMPTZ,
    -- Время последнего хода (для начала партии - момент старта): от него считается
    -- таймаут первого хода.
    last_move_at TIMESTAMPTZ,
    finished_at TIMESTAMPTZ,

    CONSTRAINT games_status_valid
        CHECK (status IN ('waiting', 'active', 'finished', 'aborted')),
    CONSTRAINT games_result_valid
        CHECK (result IN ('white', 'black', 'draw')),
    CONSTRAINT games_initial_positive CHECK (initial_ms > 0),
    CONSTRAINT games_increment_nonnegative CHECK (increment_ms >= 0),
    CONSTRAINT games_clocks_nonnegative CHECK (white_ms >= 0 AND black_ms >= 0),
    CONSTRAINT games_players_differ CHECK (white_id <> black_id),
    CONSTRAINT games_creator_plays
        CHECK (creator_id = white_id OR creator_id = black_id),
    -- Пока нет второго игрока - это вызов; во всех остальных статусах игроки двое.
    CONSTRAINT games_waiting_has_one_player
        CHECK ((status = 'waiting') = (white_id IS NULL OR black_id IS NULL)),
    -- Результат есть ровно у законченных партий.
    CONSTRAINT games_result_iff_finished
        CHECK ((status = 'finished') = (result IS NOT NULL)),
    CONSTRAINT games_finished_at_iff_over
        CHECK ((status IN ('finished', 'aborted')) = (finished_at IS NOT NULL))
);

CREATE INDEX games_white_idx ON games (white_id, created_at DESC);
CREATE INDEX games_black_idx ON games (black_id, created_at DESC);
-- Лобби и уборка старых вызовов.
CREATE INDEX games_waiting_idx ON games (created_at) WHERE status = 'waiting';
-- Фоновая проверка флажков и таймаутов идёт только по идущим партиям.
CREATE INDEX games_active_idx ON games (id) WHERE status = 'active';
