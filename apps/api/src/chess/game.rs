//! Партия: позиция, список ходов и история для правила повторений.

use super::{
    position::{FenError, Position, PositionKey},
    rules::{Outcome, Ruleset, ruleset_by_id},
    types::{Color, Move, MoveParseError},
};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GameError {
    #[error("unknown ruleset: {0}")]
    UnknownRuleset(String),
    #[error("the game is already over")]
    GameOver,
    #[error("illegal move: {0}")]
    IllegalMove(Move),
    #[error(transparent)]
    InvalidMove(#[from] MoveParseError),
    #[error(transparent)]
    InvalidFen(#[from] FenError),
}

/// Идущая или законченная партия. Все решения о ходах принимает набор правил, партия
/// только хранит состояние и не пускает ходы после конца игры.
///
/// Источник истины - начальная позиция и список ходов: по ним партию можно
/// восстановить ([`Game::from_moves`]), поэтому в БД достаточно хранить `ruleset`,
/// начальный FEN (если он нестандартный) и ходы.
pub struct Game {
    rules: Box<dyn Ruleset>,
    position: Position,
    moves: Vec<Move>,
    /// Ключи всех позиций: `history[0]` - начальная, `history[n]` - после n-го хода.
    history: Vec<PositionKey>,
    outcome: Option<Outcome>,
}

impl Game {
    /// Новая партия с начальной позицией набора правил.
    pub fn new(ruleset_id: &str) -> Result<Game, GameError> {
        let rules = rules_for(ruleset_id)?;
        let position = rules.initial_position();

        Ok(Game::with_position(rules, position))
    }

    /// Партия с произвольной начальной позицией (тесты, расстановки, будущие режимы).
    pub fn from_fen(ruleset_id: &str, fen: &str) -> Result<Game, GameError> {
        let rules = rules_for(ruleset_id)?;
        let position = Position::from_fen(fen)?;

        Ok(Game::with_position(rules, position))
    }

    /// Восстанавливает партию, проигрывая ходы с начальной позиции набора правил.
    pub fn from_moves(ruleset_id: &str, moves: &[Move]) -> Result<Game, GameError> {
        let mut game = Game::new(ruleset_id)?;
        for &mv in moves {
            game.play(mv)?;
        }

        Ok(game)
    }

    fn with_position(rules: Box<dyn Ruleset>, position: Position) -> Game {
        let history = vec![rules.position_key(&position)];
        let outcome = rules.outcome(&position, &history);

        Game {
            rules,
            position,
            moves: Vec::new(),
            history,
            outcome,
        }
    }

    /// Играет ход. Возвращает исход партии после хода (`None` - игра продолжается).
    pub fn play(&mut self, mv: Move) -> Result<Option<Outcome>, GameError> {
        if self.outcome.is_some() {
            return Err(GameError::GameOver);
        }
        if !self.rules.legal_moves(&self.position).contains(&mv) {
            return Err(GameError::IllegalMove(mv));
        }

        self.position = self.rules.apply(&self.position, mv);
        self.moves.push(mv);
        self.history.push(self.rules.position_key(&self.position));
        self.outcome = self.rules.outcome(&self.position, &self.history);

        Ok(self.outcome)
    }

    /// То же, что [`Game::play`], для хода в записи UCI (`e2e4`, `e7e8q`).
    pub fn play_uci(&mut self, text: &str) -> Result<Option<Outcome>, GameError> {
        self.play(Move::from_uci(text)?)
    }

    pub fn legal_moves(&self) -> Vec<Move> {
        if self.outcome.is_some() {
            return Vec::new();
        }

        self.rules.legal_moves(&self.position)
    }

    pub fn ruleset_id(&self) -> &'static str {
        self.rules.id()
    }

    pub fn position(&self) -> &Position {
        &self.position
    }

    pub fn fen(&self) -> String {
        self.position.to_fen()
    }

    /// Чей сейчас ход.
    pub fn turn(&self) -> Color {
        self.position.turn
    }

    pub fn moves(&self) -> &[Move] {
        &self.moves
    }

    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    pub fn is_over(&self) -> bool {
        self.outcome.is_some()
    }
}

fn rules_for(ruleset_id: &str) -> Result<Box<dyn Ruleset>, GameError> {
    ruleset_by_id(ruleset_id).ok_or_else(|| GameError::UnknownRuleset(ruleset_id.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chess::{rules::STANDARD, rules::Termination};

    fn play_all(game: &mut Game, moves: &[&str]) -> Option<Outcome> {
        let mut outcome = None;
        for text in moves {
            outcome = game.play_uci(text).expect("move is legal");
        }
        outcome
    }

    #[test]
    fn new_game_starts_from_the_standard_position() {
        let game = Game::new(STANDARD).unwrap();

        assert_eq!(game.turn(), Color::White);
        assert_eq!(game.legal_moves().len(), 20);
        assert_eq!(game.outcome(), None);
        assert_eq!(game.ruleset_id(), STANDARD);
        assert!(game.moves().is_empty());
    }

    #[test]
    fn unknown_ruleset_is_an_error() {
        assert_eq!(
            Game::new("nope").err(),
            Some(GameError::UnknownRuleset("nope".to_string()))
        );
    }

    #[test]
    fn plays_moves_and_alternates_turns() {
        let mut game = Game::new(STANDARD).unwrap();

        assert_eq!(play_all(&mut game, &["e2e4", "e7e5", "g1f3"]), None);
        assert_eq!(game.turn(), Color::Black);
        assert_eq!(game.moves().len(), 3);
        assert_eq!(
            game.fen(),
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R b KQkq - 1 2"
        );
    }

    #[test]
    fn rejects_illegal_moves_without_changing_state() {
        let mut game = Game::new(STANDARD).unwrap();
        let before = game.fen();

        for text in ["e2e5", "e7e5", "a1a3", "e1e2", "d2d4q"] {
            assert!(
                matches!(game.play_uci(text), Err(GameError::IllegalMove(_))),
                "{text}"
            );
        }
        assert!(matches!(
            game.play_uci("garbage"),
            Err(GameError::InvalidMove(_))
        ));
        assert_eq!(game.fen(), before);
        assert!(game.moves().is_empty());
    }

    #[test]
    fn promotion_must_name_the_piece() {
        let mut game = Game::from_fen(STANDARD, "8/P7/8/8/8/8/8/k6K w - - 0 1").unwrap();

        assert!(matches!(
            game.play_uci("a7a8"),
            Err(GameError::IllegalMove(_))
        ));
        assert_eq!(game.play_uci("a7a8q"), Ok(None));
    }

    #[test]
    fn fools_mate_ends_the_game() {
        let mut game = Game::new(STANDARD).unwrap();
        let outcome = play_all(&mut game, &["f2f3", "e7e5", "g2g4", "d8h4"]);

        assert_eq!(
            outcome,
            Some(Outcome::win(Color::Black, Termination::Checkmate))
        );
        assert!(game.is_over());
        assert!(game.legal_moves().is_empty());
        assert_eq!(game.play_uci("a2a3"), Err(GameError::GameOver));
    }

    #[test]
    fn stalemate_position_is_a_draw_from_the_start() {
        let game = Game::from_fen(STANDARD, "7k/5Q2/6K1/8/8/8/8/8 b - - 0 1").unwrap();

        assert_eq!(game.outcome(), Some(Outcome::draw(Termination::Stalemate)));
    }

    #[test]
    fn threefold_repetition_is_a_draw() {
        let mut game = Game::new(STANDARD).unwrap();
        let shuffle = ["g1f3", "g8f6", "f3g1", "f6g8"];

        // Начальная позиция видна в третий раз только после второго круга.
        assert_eq!(play_all(&mut game, &shuffle), None);
        assert_eq!(
            play_all(&mut game, &shuffle),
            Some(Outcome::draw(Termination::ThreefoldRepetition))
        );
        assert_eq!(game.moves().len(), 8);
    }

    #[test]
    fn castling_rights_change_the_position_for_repetition() {
        // После хода королём туда-обратно права потеряны: позиция не та же самая.
        let mut game = Game::new(STANDARD).unwrap();
        let moves = [
            "e2e4", "e7e5", "e1e2", "e8e7", "e2e1", "e7e8", "e1e2", "e8e7", "e2e1", "e7e8",
        ];

        assert_eq!(play_all(&mut game, &moves), None);
        assert!(!game.is_over());
    }

    #[test]
    fn replay_reproduces_the_same_game() {
        let mut original = Game::new(STANDARD).unwrap();
        play_all(
            &mut original,
            &[
                "e2e4", "c7c5", "g1f3", "d7d6", "d2d4", "c5d4", "f3d4", "g8f6",
            ],
        );

        let replayed = Game::from_moves(STANDARD, original.moves()).unwrap();

        assert_eq!(replayed.fen(), original.fen());
        assert_eq!(replayed.moves(), original.moves());
        assert_eq!(replayed.outcome(), original.outcome());
    }

    #[test]
    fn replay_fails_on_an_illegal_history() {
        let moves = [
            Move::from_uci("e2e4").unwrap(),
            Move::from_uci("e2e4").unwrap(),
        ];

        assert!(matches!(
            Game::from_moves(STANDARD, &moves),
            Err(GameError::IllegalMove(_))
        ));
    }
}
