//! Базовые типы: цвет, фигура, клетка, ход.

use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub const fn opposite(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }

    /// Куда идут пешки по рангам: +1 у белых, -1 у чёрных.
    pub const fn pawn_direction(self) -> i8 {
        match self {
            Color::White => 1,
            Color::Black => -1,
        }
    }

    /// Ранг, на котором стоят фигуры в начале партии (0 - первый ранг).
    pub const fn home_rank(self) -> u8 {
        match self {
            Color::White => 0,
            Color::Black => 7,
        }
    }

    /// Ранг, с которого пешка может пойти на две клетки.
    pub const fn pawn_start_rank(self) -> u8 {
        match self {
            Color::White => 1,
            Color::Black => 6,
        }
    }

    /// Ранг превращения пешки.
    pub const fn promotion_rank(self) -> u8 {
        self.opposite().home_rank()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PieceKind {
    Pawn,
    Knight,
    Bishop,
    Rook,
    Queen,
    King,
}

impl PieceKind {
    /// Буква в нижнем регистре (как в UCI и FEN для чёрных).
    pub const fn letter(self) -> char {
        match self {
            PieceKind::Pawn => 'p',
            PieceKind::Knight => 'n',
            PieceKind::Bishop => 'b',
            PieceKind::Rook => 'r',
            PieceKind::Queen => 'q',
            PieceKind::King => 'k',
        }
    }

    pub const fn from_letter(letter: char) -> Option<PieceKind> {
        match letter {
            'p' => Some(PieceKind::Pawn),
            'n' => Some(PieceKind::Knight),
            'b' => Some(PieceKind::Bishop),
            'r' => Some(PieceKind::Rook),
            'q' => Some(PieceKind::Queen),
            'k' => Some(PieceKind::King),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Piece {
    pub color: Color,
    pub kind: PieceKind,
}

impl Piece {
    pub const fn new(color: Color, kind: PieceKind) -> Self {
        Self { color, kind }
    }

    /// Символ для FEN: заглавная - белые, строчная - чёрные.
    pub const fn fen_char(self) -> char {
        let letter = self.kind.letter();
        match self.color {
            Color::White => letter.to_ascii_uppercase(),
            Color::Black => letter,
        }
    }

    pub const fn from_fen_char(c: char) -> Option<Piece> {
        let Some(kind) = PieceKind::from_letter(c.to_ascii_lowercase()) else {
            return None;
        };
        let color = if c.is_ascii_uppercase() {
            Color::White
        } else {
            Color::Black
        };

        Some(Piece::new(color, kind))
    }
}

/// Клетка доски. Номер = ранг * 8 + вертикаль, то есть a1 = 0, h1 = 7, a8 = 56, h8 = 63.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Square(u8);

impl Square {
    /// `file` и `rank` считаются с нуля: вертикаль a = 0, первый ранг = 0.
    pub const fn new(file: u8, rank: u8) -> Option<Square> {
        if file < 8 && rank < 8 {
            Some(Square(rank * 8 + file))
        } else {
            None
        }
    }

    /// То же, что [`Square::new`], но для заведомо верных координат (константы в правилах).
    /// Паникует, если координаты вне доски.
    pub const fn at(file: u8, rank: u8) -> Square {
        assert!(file < 8 && rank < 8, "square is outside the board");
        Square(rank * 8 + file)
    }

    pub const fn index(self) -> usize {
        self.0 as usize
    }

    pub const fn file(self) -> u8 {
        self.0 % 8
    }

    pub const fn rank(self) -> u8 {
        self.0 / 8
    }

    /// Все 64 клетки от a1 до h8.
    pub fn all() -> impl Iterator<Item = Square> {
        (0u8..64).map(Square)
    }

    /// Сдвиг на `df` вертикалей и `dr` рангов; `None`, если вышли за доску.
    pub fn offset(self, df: i8, dr: i8) -> Option<Square> {
        let file = i8::try_from(self.file()).ok()? + df;
        let rank = i8::try_from(self.rank()).ok()? + dr;

        Square::new(u8::try_from(file).ok()?, u8::try_from(rank).ok()?)
    }

    /// Разбор записи вида `e4`.
    pub fn parse(text: &str) -> Option<Square> {
        let bytes = text.as_bytes();
        if bytes.len() != 2 {
            return None;
        }

        Square::new(bytes[0].checked_sub(b'a')?, bytes[1].checked_sub(b'1')?)
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}{}",
            char::from(b'a' + self.file()),
            char::from(b'1' + self.rank())
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid move notation {0:?} (expected something like e2e4 or e7e8q)")]
pub struct MoveParseError(pub String);

/// Ход в формате UCI: откуда, куда и (для превращения пешки) в кого.
///
/// Рокировка - это ход короля на две клетки (`e1g1`), взятие на проходе - ход пешки
/// на пустое поле, названное в `en_passant` позиции. Специальные случаи определяет
/// сам набор правил по позиции, поэтому формат хода остаётся простым.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Move {
    pub from: Square,
    pub to: Square,
    pub promotion: Option<PieceKind>,
}

impl Move {
    pub const fn new(from: Square, to: Square) -> Self {
        Self {
            from,
            to,
            promotion: None,
        }
    }

    pub const fn with_promotion(from: Square, to: Square, kind: PieceKind) -> Self {
        Self {
            from,
            to,
            promotion: Some(kind),
        }
    }

    /// Разбор UCI-записи: `e2e4`, `e7e8q`.
    pub fn from_uci(text: &str) -> Result<Move, MoveParseError> {
        let invalid = || MoveParseError(text.to_string());

        if !text.is_ascii() || !(4..=5).contains(&text.len()) {
            return Err(invalid());
        }

        let from = Square::parse(&text[0..2]).ok_or_else(invalid)?;
        let to = Square::parse(&text[2..4]).ok_or_else(invalid)?;

        let promotion = match text[4..].chars().next() {
            None => None,
            Some(c) => match PieceKind::from_letter(c.to_ascii_lowercase()) {
                Some(
                    kind @ (PieceKind::Queen
                    | PieceKind::Rook
                    | PieceKind::Bishop
                    | PieceKind::Knight),
                ) => Some(kind),
                _ => return Err(invalid()),
            },
        };

        Ok(Move {
            from,
            to,
            promotion,
        })
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.from, self.to)?;
        if let Some(kind) = self.promotion {
            write!(f, "{}", kind.letter())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_roundtrip() {
        assert_eq!(Square::at(0, 0).to_string(), "a1");
        assert_eq!(Square::at(7, 7).to_string(), "h8");
        assert_eq!(Square::parse("e4"), Square::new(4, 3));

        for square in Square::all() {
            assert_eq!(Square::parse(&square.to_string()), Some(square));
        }
        assert_eq!(Square::all().count(), 64);
    }

    #[test]
    fn square_parse_rejects_garbage() {
        for text in ["", "e", "e44", "i1", "a9", "a0", "E4", "4e", "é4"] {
            assert_eq!(Square::parse(text), None, "{text:?}");
        }
    }

    #[test]
    fn square_offset_stays_on_board() {
        let a1 = Square::at(0, 0);
        assert_eq!(a1.offset(-1, 0), None);
        assert_eq!(a1.offset(0, -1), None);
        assert_eq!(a1.offset(1, 2), Square::new(1, 2));
        assert_eq!(Square::at(7, 7).offset(1, 1), None);
    }

    #[test]
    fn uci_roundtrip() {
        for text in ["e2e4", "g1f3", "e7e8q", "a2a1n", "h7h8r", "b7b8b"] {
            assert_eq!(Move::from_uci(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn uci_rejects_garbage() {
        for text in [
            "", "e2", "e2e", "e2e4x", "e2e4qq", "e7e8k", "e7e8p", "z2e4", "e2e9", "е2е4",
        ] {
            assert!(Move::from_uci(text).is_err(), "{text:?}");
        }
    }

    #[test]
    fn piece_fen_chars() {
        assert_eq!(
            Piece::from_fen_char('N'),
            Some(Piece::new(Color::White, PieceKind::Knight))
        );
        assert_eq!(
            Piece::from_fen_char('q'),
            Some(Piece::new(Color::Black, PieceKind::Queen))
        );
        assert_eq!(Piece::from_fen_char('x'), None);
        assert_eq!(Piece::new(Color::White, PieceKind::King).fen_char(), 'K');
    }
}
