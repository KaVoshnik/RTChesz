//! Шахматные часы. Модуль чистый: время всегда передаётся аргументом.
//!
//! Правила:
//! * Хранится остаток каждой стороны на момент `running_since`. Тикают только часы стороны,
//!   которая ходит, и только если `running_since` задан.
//! * Часы запускаются, когда оба игрока сделали по первому ходу. До этого время не идёт,
//!   но на первый ход даётся ограничение (`GAME_FIRST_MOVE_TIMEOUT_SECS`), иначе партия отменяется.
//! * После хода, сделанного при идущих часах, к остатку игрока добавляется инкремент.
//!   Первый ход каждой стороны инкремента не даёт.

use chrono::{DateTime, Utc};

use crate::{chess::Color, error::AppError};

pub const MIN_INITIAL_SECS: i64 = 15;
pub const MAX_INITIAL_SECS: i64 = 3 * 60 * 60;
pub const MAX_INCREMENT_SECS: i64 = 180;

/// Состояние часов обеих сторон.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clocks {
    pub white_ms: i64,
    pub black_ms: i64,
    /// С какого момента тикают часы стороны, чей сейчас ход. `None` - часы стоят.
    pub running_since: Option<DateTime<Utc>>,
}

impl Clocks {
    fn stored(&self, color: Color) -> i64 {
        match color {
            Color::White => self.white_ms,
            Color::Black => self.black_ms,
        }
    }

    /// Остаток времени стороны `color` в момент `now`; `turn` - чей сейчас ход.
    /// Не бывает отрицательным.
    pub fn remaining(&self, color: Color, turn: Color, now: DateTime<Utc>) -> i64 {
        let stored = self.stored(color);

        match self.running_since {
            Some(since) if color == turn => (stored - elapsed_ms(since, now)).max(0),
            _ => stored,
        }
    }

    /// У стороны, которая ходит, время вышло.
    pub fn is_flagged(&self, turn: Color, now: DateTime<Utc>) -> bool {
        self.running_since.is_some() && self.remaining(turn, turn, now) == 0
    }

    /// Часы после хода стороны `mover` в момент `now`. `plies_before` - сколько полуходов
    /// было сыграно до этого хода. `None` - у `mover` время уже вышло, ход не засчитывается.
    pub fn after_move(
        &self,
        mover: Color,
        plies_before: usize,
        increment_ms: i64,
        now: DateTime<Utc>,
    ) -> Option<Clocks> {
        let mut next = *self;

        if self.running_since.is_some() {
            let left = self.remaining(mover, mover, now);
            if left == 0 {
                return None;
            }
            let left = left + increment_ms;
            match mover {
                Color::White => next.white_ms = left,
                Color::Black => next.black_ms = left,
            }
        }

        // После второго полухода (первый ход чёрных) часы белых запускаются.
        next.running_since = (plies_before + 1 >= 2).then_some(now);

        Some(next)
    }

    /// Сторона `color` потеряла всё время (флажок): её остаток обнуляется.
    pub fn flagged(&self, color: Color) -> Clocks {
        let mut next = *self;
        match color {
            Color::White => next.white_ms = 0,
            Color::Black => next.black_ms = 0,
        }
        next.running_since = None;
        next
    }
}

fn elapsed_ms(since: DateTime<Utc>, now: DateTime<Utc>) -> i64 {
    (now - since).num_milliseconds().max(0)
}

/// Контроль времени из запроса: секунды -> миллисекунды, с проверкой границ.
pub fn validate_time_control(
    initial_secs: i64,
    increment_secs: i64,
) -> Result<(i64, i64), AppError> {
    if !(MIN_INITIAL_SECS..=MAX_INITIAL_SECS).contains(&initial_secs) {
        return Err(AppError::bad_request(format!(
            "Время на партию должно быть от {MIN_INITIAL_SECS} секунд до {} минут",
            MAX_INITIAL_SECS / 60
        )));
    }
    if !(0..=MAX_INCREMENT_SECS).contains(&increment_secs) {
        return Err(AppError::bad_request(format!(
            "Добавление за ход должно быть от 0 до {MAX_INCREMENT_SECS} секунд"
        )));
    }

    Ok((initial_secs * 1000, increment_secs * 1000))
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn t(secs: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_800_000_000 + secs, 0).unwrap()
    }

    fn clocks(running: Option<DateTime<Utc>>) -> Clocks {
        Clocks {
            white_ms: 60_000,
            black_ms: 60_000,
            running_since: running,
        }
    }

    #[test]
    fn stopped_clocks_do_not_tick() {
        let c = clocks(None);

        assert_eq!(c.remaining(Color::White, Color::White, t(1000)), 60_000);
        assert!(!c.is_flagged(Color::White, t(1000)));
    }

    #[test]
    fn only_side_to_move_ticks() {
        let c = clocks(Some(t(0)));

        assert_eq!(c.remaining(Color::White, Color::White, t(10)), 50_000);
        assert_eq!(c.remaining(Color::Black, Color::White, t(10)), 60_000);
    }

    #[test]
    fn remaining_never_goes_negative_and_flags() {
        let c = clocks(Some(t(0)));

        assert_eq!(c.remaining(Color::White, Color::White, t(500)), 0);
        assert!(c.is_flagged(Color::White, t(60)));
        assert!(!c.is_flagged(Color::White, t(59)));
        // Часы стороны, которая не ходит, не тикают: у чёрных остаток прежний.
        assert_eq!(c.remaining(Color::Black, Color::White, t(500)), 60_000);
    }

    #[test]
    fn first_two_plies_are_free_and_start_clocks_after_black() {
        let c = clocks(None);

        // Первый ход белых: часы по-прежнему стоят, инкремента нет.
        let c = c.after_move(Color::White, 0, 2000, t(100)).unwrap();
        assert_eq!((c.white_ms, c.black_ms), (60_000, 60_000));
        assert_eq!(c.running_since, None);

        // Первый ход чёрных: часы белых запускаются.
        let c = c.after_move(Color::Black, 1, 2000, t(200)).unwrap();
        assert_eq!((c.white_ms, c.black_ms), (60_000, 60_000));
        assert_eq!(c.running_since, Some(t(200)));
    }

    #[test]
    fn move_spends_time_and_adds_increment() {
        let c = clocks(Some(t(0)));

        let next = c.after_move(Color::White, 2, 3000, t(10)).unwrap();
        // 60 - 10 + 3.
        assert_eq!(next.white_ms, 53_000);
        assert_eq!(next.black_ms, 60_000);
        assert_eq!(next.running_since, Some(t(10)));
    }

    #[test]
    fn move_after_flag_is_rejected() {
        let c = clocks(Some(t(0)));

        assert_eq!(c.after_move(Color::White, 2, 3000, t(61)), None);
    }

    #[test]
    fn flagged_zeroes_the_loser_and_stops_clocks() {
        let c = clocks(Some(t(0))).flagged(Color::White);

        assert_eq!((c.white_ms, c.black_ms), (0, 60_000));
        assert_eq!(c.running_since, None);
    }

    #[test]
    fn time_control_bounds() {
        assert_eq!(validate_time_control(300, 3).unwrap(), (300_000, 3_000));
        assert!(validate_time_control(14, 0).is_err());
        assert!(validate_time_control(MAX_INITIAL_SECS + 1, 0).is_err());
        assert!(validate_time_control(300, -1).is_err());
        assert!(validate_time_control(300, MAX_INCREMENT_SECS + 1).is_err());
    }
}
