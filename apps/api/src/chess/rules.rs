//! Набор правил: всё, что отличает одни шахматы от других, стоит за трейтом [`Ruleset`].

use super::{
    position::{Position, PositionKey},
    standard::StandardRules,
    types::{Color, Move},
};

/// Идентификатор обычных шахмат (значение колонки `games.ruleset`).
pub const STANDARD: &str = "standard";

/// Чем кончилась партия по правилам. Сдача, согласие на ничью и вылет по времени сюда
/// не входят: это решения игроков и часов, их фиксирует сервис партий.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Termination {
    Checkmate,
    Stalemate,
    InsufficientMaterial,
    FiftyMoveRule,
    ThreefoldRepetition,
}

impl Termination {
    /// Стабильное имя для БД и API.
    pub const fn as_str(self) -> &'static str {
        match self {
            Termination::Checkmate => "checkmate",
            Termination::Stalemate => "stalemate",
            Termination::InsufficientMaterial => "insufficient_material",
            Termination::FiftyMoveRule => "fifty_move_rule",
            Termination::ThreefoldRepetition => "threefold_repetition",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Outcome {
    /// `None` - ничья.
    pub winner: Option<Color>,
    pub termination: Termination,
}

impl Outcome {
    pub const fn win(winner: Color, termination: Termination) -> Self {
        Self {
            winner: Some(winner),
            termination,
        }
    }

    pub const fn draw(termination: Termination) -> Self {
        Self {
            winner: None,
            termination,
        }
    }
}

/// Правила игры.
///
/// Трейт держит минимум: какие ходы разрешены, как ход меняет позицию, как опознаётся
/// одинаковая позиция и когда игра кончилась. Партия ([`super::Game`]) больше ничего
/// о шахматах не знает, поэтому новый набор правил = новая реализация этого трейта.
pub trait Ruleset: Send + Sync {
    /// Стабильный идентификатор набора правил.
    fn id(&self) -> &'static str;

    fn initial_position(&self) -> Position;

    /// Все разрешённые ходы стороне, которая ходит в `position`.
    fn legal_moves(&self, position: &Position) -> Vec<Move>;

    /// Позиция после хода. Ход должен быть из [`Ruleset::legal_moves`] этой же позиции:
    /// для других ходов результат не определён (но паники нет).
    fn apply(&self, position: &Position, mv: Move) -> Position;

    /// Ключ для поиска повторений.
    fn position_key(&self, position: &Position) -> PositionKey;

    /// Закончилась ли партия. `history` - ключи всех позиций партии по порядку, *включая*
    /// текущую (она последняя).
    fn outcome(&self, position: &Position, history: &[PositionKey]) -> Option<Outcome>;
}

/// Находит набор правил по идентификатору из БД.
pub fn ruleset_by_id(id: &str) -> Option<Box<dyn Ruleset>> {
    match id {
        STANDARD => Some(Box::new(StandardRules::default())),
        _ => None,
    }
}

/// Считает листья дерева ходов на глубину `depth`. Стандартный способ проверить генератор
/// ходов: числа для известных позиций опубликованы, любая ошибка в правилах их меняет.
pub fn perft(rules: &dyn Ruleset, position: &Position, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }

    let moves = rules.legal_moves(position);
    if depth == 1 {
        return moves.len() as u64;
    }

    moves
        .into_iter()
        .map(|mv| perft(rules, &rules.apply(position, mv), depth - 1))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_ruleset_is_registered() {
        let rules = ruleset_by_id(STANDARD).expect("standard rules exist");
        assert_eq!(rules.id(), STANDARD);
        assert!(ruleset_by_id("no-such-rules").is_none());
    }

    #[test]
    fn termination_names_are_stable() {
        assert_eq!(Termination::Checkmate.as_str(), "checkmate");
        assert_eq!(
            Termination::ThreefoldRepetition.as_str(),
            "threefold_repetition"
        );
    }
}
