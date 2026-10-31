//! Шахматная логика: правила, позиция, партия.
//!
//! Модуль чистый: ни БД, ни сети, ни часов - только данные и правила, поэтому его легко
//! тестировать и менять.
//!
//! # Как это устроено
//!
//! * [`types`] - цвет, фигура, клетка, ход (формат хода совпадает с UCI: `e2e4`, `e7e8q`).
//! * [`position`] - позиция на доске и FEN.
//! * [`rules`] - трейт [`Ruleset`]: всё, что можно менять в правилах, стоит за ним.
//! * [`standard`] - обычные шахматы ([`StandardRules`]). Здесь же лежат переиспользуемые
//!   кирпичики (генерация ходов, проверка атаки, применение хода).
//! * [`game`] - партия: позиция + список ходов + история для повторений.
//!
//! # Как менять правила
//!
//! 1. Мелкая правка (например, другое условие ничьей): поля `StandardRules` или своя
//!    реализация `outcome`.
//! 2. Своя фигура или особый ход: добавить генерацию в свою реализацию `legal_moves`/`apply`,
//!    переиспользуя функции из [`standard`].
//! 3. Новый набор правил: реализовать [`Ruleset`], зарегистрировать id в
//!    [`rules::ruleset_by_id`] и записывать этот id в партию (колонка `games.ruleset`).
//!
//! Источник истины для партии - начальная позиция + список ходов: любую партию можно
//! воспроизвести через [`Game::from_moves`] с тем набором правил, по которому она шла.

pub mod game;
pub mod position;
pub mod rules;
pub mod standard;
pub mod types;

pub use game::{Game, GameError};
pub use position::{CastlingRights, FenError, Position, PositionKey};
pub use rules::{Outcome, Ruleset, Termination, ruleset_by_id};
pub use standard::StandardRules;
pub use types::{Color, Move, MoveParseError, Piece, PieceKind, Square};
