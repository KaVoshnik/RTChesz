//! Позиция на доске и работа с FEN.

use super::types::{Color, Piece, PieceKind, Square};

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Права на рокировку. Хранят только "ещё не терялось": свободны ли поля и бьются ли они,
/// проверяют правила при генерации ходов.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CastlingRights {
    pub white_kingside: bool,
    pub white_queenside: bool,
    pub black_kingside: bool,
    pub black_queenside: bool,
}

impl CastlingRights {
    pub const ALL: CastlingRights = CastlingRights {
        white_kingside: true,
        white_queenside: true,
        black_kingside: true,
        black_queenside: true,
    };

    pub const fn kingside(self, color: Color) -> bool {
        match color {
            Color::White => self.white_kingside,
            Color::Black => self.black_kingside,
        }
    }

    pub const fn queenside(self, color: Color) -> bool {
        match color {
            Color::White => self.white_queenside,
            Color::Black => self.black_queenside,
        }
    }

    /// Цвет потерял оба права (король сходил).
    pub fn clear_color(&mut self, color: Color) {
        match color {
            Color::White => {
                self.white_kingside = false;
                self.white_queenside = false;
            }
            Color::Black => {
                self.black_kingside = false;
                self.black_queenside = false;
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FenError {
    #[error("FEN must have 4 to 6 fields, got {0}")]
    FieldCount(usize),
    #[error("invalid piece placement: {0}")]
    Board(&'static str),
    #[error("invalid side to move: {0:?}")]
    Turn(String),
    #[error("invalid castling rights: {0:?}")]
    Castling(String),
    #[error("invalid en passant square: {0:?}")]
    EnPassant(String),
    #[error("invalid move counter: {0:?}")]
    Counter(String),
    #[error("invalid position: {0}")]
    Position(&'static str),
}

/// Позиция: расстановка + всё, что нужно, чтобы продолжить партию.
///
/// Поля открыты намеренно: набор правил может хранить и менять их как ему нужно.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Position {
    /// Клетки в порядке [`Square::index`]: a1, b1, ..., h1, a2, ..., h8.
    pub board: [Option<Piece>; 64],
    pub turn: Color,
    pub castling: CastlingRights,
    /// Клетка, "перепрыгнутая" пешкой при последнем ходе на две клетки (как в FEN).
    pub en_passant: Option<Square>,
    /// Полуходы с последнего взятия или хода пешки (для правила 50 ходов).
    pub halfmove_clock: u32,
    pub fullmove_number: u32,
}

impl Position {
    /// Обычная начальная позиция.
    pub fn startpos() -> Position {
        Position::from_fen(START_FEN).expect("start position FEN is valid")
    }

    pub fn piece_at(&self, square: Square) -> Option<Piece> {
        self.board[square.index()]
    }

    pub fn set_piece(&mut self, square: Square, piece: Option<Piece>) {
        self.board[square.index()] = piece;
    }

    pub fn king_square(&self, color: Color) -> Option<Square> {
        Square::all()
            .find(|&square| self.piece_at(square) == Some(Piece::new(color, PieceKind::King)))
    }

    /// Разбирает FEN. Проверяет, что запись самосогласована (по одному королю, пешки не на
    /// крайних рангах, права на рокировку и взятие на проходе соответствуют доске), но не
    /// проверяет достижимость позиции из начальной.
    pub fn from_fen(fen: &str) -> Result<Position, FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if !(4..=6).contains(&fields.len()) {
            return Err(FenError::FieldCount(fields.len()));
        }

        let board = parse_board(fields[0])?;

        let turn = match fields[1] {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(FenError::Turn(other.to_string())),
        };

        let castling = parse_castling(fields[2])?;

        let en_passant = match fields[3] {
            "-" => None,
            text => Some(Square::parse(text).ok_or_else(|| FenError::EnPassant(text.to_string()))?),
        };

        let halfmove_clock = parse_counter(fields.get(4).copied(), 0)?;
        let fullmove_number = parse_counter(fields.get(5).copied(), 1)?;

        let position = Position {
            board,
            turn,
            castling,
            en_passant,
            halfmove_clock,
            fullmove_number,
        };
        position.validate()?;

        Ok(position)
    }

    pub fn to_fen(&self) -> String {
        let mut fen = String::new();

        for rank in (0..8u8).rev() {
            let mut empty = 0;
            for file in 0..8u8 {
                match self.piece_at(Square::at(file, rank)) {
                    Some(piece) => {
                        if empty > 0 {
                            fen.push_str(&empty.to_string());
                            empty = 0;
                        }
                        fen.push(piece.fen_char());
                    }
                    None => empty += 1,
                }
            }
            if empty > 0 {
                fen.push_str(&empty.to_string());
            }
            if rank > 0 {
                fen.push('/');
            }
        }

        fen.push(' ');
        fen.push(match self.turn {
            Color::White => 'w',
            Color::Black => 'b',
        });

        fen.push(' ');
        let rights = [
            (self.castling.white_kingside, 'K'),
            (self.castling.white_queenside, 'Q'),
            (self.castling.black_kingside, 'k'),
            (self.castling.black_queenside, 'q'),
        ];
        let castling: String = rights
            .iter()
            .filter(|(has, _)| *has)
            .map(|(_, letter)| *letter)
            .collect();
        fen.push_str(if castling.is_empty() { "-" } else { &castling });

        fen.push(' ');
        match self.en_passant {
            Some(square) => fen.push_str(&square.to_string()),
            None => fen.push('-'),
        }

        fen.push_str(&format!(
            " {} {}",
            self.halfmove_clock, self.fullmove_number
        ));

        fen
    }

    fn validate(&self) -> Result<(), FenError> {
        for color in [Color::White, Color::Black] {
            let kings = Square::all()
                .filter(|&sq| self.piece_at(sq) == Some(Piece::new(color, PieceKind::King)))
                .count();
            if kings != 1 {
                return Err(FenError::Position("each side must have exactly one king"));
            }
        }

        let pawn_on_edge = Square::all().any(|sq| {
            matches!(self.piece_at(sq), Some(p) if p.kind == PieceKind::Pawn)
                && (sq.rank() == 0 || sq.rank() == 7)
        });
        if pawn_on_edge {
            return Err(FenError::Position(
                "pawns cannot stand on the first or last rank",
            ));
        }

        for color in [Color::White, Color::Black] {
            let home = color.home_rank();
            let king = Piece::new(color, PieceKind::King);
            let rook = Piece::new(color, PieceKind::Rook);
            let king_home = self.piece_at(Square::at(4, home)) == Some(king);

            let kingside_ok = !self.castling.kingside(color)
                || (king_home && self.piece_at(Square::at(7, home)) == Some(rook));
            let queenside_ok = !self.castling.queenside(color)
                || (king_home && self.piece_at(Square::at(0, home)) == Some(rook));

            if !kingside_ok || !queenside_ok {
                return Err(FenError::Castling(
                    "rights do not match king and rook placement".to_string(),
                ));
            }
        }

        if let Some(target) = self.en_passant {
            // Цель взятия на проходе стоит за только что прошедшей пешкой противника
            // того, чей сейчас не ход.
            let mover = self.turn.opposite();
            let expected_rank = match mover {
                Color::White => 2,
                Color::Black => 5,
            };
            let pawn_square = target.offset(0, mover.pawn_direction());
            let origin_square = target.offset(0, -mover.pawn_direction());

            let valid = target.rank() == expected_rank
                && self.piece_at(target).is_none()
                && pawn_square.is_some_and(|sq| {
                    self.piece_at(sq) == Some(Piece::new(mover, PieceKind::Pawn))
                })
                && origin_square.is_some_and(|sq| self.piece_at(sq).is_none());

            if !valid {
                return Err(FenError::EnPassant(target.to_string()));
            }
        }

        Ok(())
    }
}

fn parse_board(text: &str) -> Result<[Option<Piece>; 64], FenError> {
    let ranks: Vec<&str> = text.split('/').collect();
    if ranks.len() != 8 {
        return Err(FenError::Board("expected 8 ranks"));
    }

    let mut board = [None; 64];

    for (row, rank_text) in ranks.iter().enumerate() {
        // Первая строка FEN - восьмой ранг.
        let rank = u8::try_from(7 - row).map_err(|_| FenError::Board("too many ranks"))?;
        let mut file = 0u8;

        for c in rank_text.chars() {
            if let Some(skip) = c.to_digit(10) {
                if !(1..=8).contains(&skip) {
                    return Err(FenError::Board("empty-square count must be 1-8"));
                }
                file += skip as u8;
                if file > 8 {
                    return Err(FenError::Board("rank has more than 8 squares"));
                }
            } else {
                let piece = Piece::from_fen_char(c).ok_or(FenError::Board("unknown piece"))?;
                if file >= 8 {
                    return Err(FenError::Board("rank has more than 8 squares"));
                }
                board[Square::at(file, rank).index()] = Some(piece);
                file += 1;
            }
        }

        if file != 8 {
            return Err(FenError::Board("rank must describe exactly 8 squares"));
        }
    }

    Ok(board)
}

fn parse_castling(text: &str) -> Result<CastlingRights, FenError> {
    let mut rights = CastlingRights::default();
    if text == "-" {
        return Ok(rights);
    }

    for c in text.chars() {
        let slot = match c {
            'K' => &mut rights.white_kingside,
            'Q' => &mut rights.white_queenside,
            'k' => &mut rights.black_kingside,
            'q' => &mut rights.black_queenside,
            _ => return Err(FenError::Castling(text.to_string())),
        };
        if *slot {
            return Err(FenError::Castling(text.to_string()));
        }
        *slot = true;
    }

    Ok(rights)
}

fn parse_counter(text: Option<&str>, default: u32) -> Result<u32, FenError> {
    match text {
        None => Ok(default),
        Some(text) => text
            .parse()
            .map_err(|_| FenError::Counter(text.to_string())),
    }
}

/// Ключ позиции для правила повторений: расстановка, чей ход, права на рокировку и
/// *реально возможное* взятие на проходе. Счётчики ходов в ключ не входят.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PositionKey {
    board: [Option<Piece>; 64],
    turn: Color,
    castling: CastlingRights,
    en_passant: Option<Square>,
}

impl PositionKey {
    /// `en_passant` - цель взятия на проходе, только если взятие действительно можно
    /// сыграть (набор правил решает это сам, см. `Ruleset::position_key`).
    pub fn new(position: &Position, en_passant: Option<Square>) -> Self {
        Self {
            board: position.board,
            turn: position.turn,
            castling: position.castling,
            en_passant,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_position_layout() {
        let position = Position::startpos();

        assert_eq!(position.turn, Color::White);
        assert_eq!(position.castling, CastlingRights::ALL);
        assert_eq!(position.en_passant, None);
        assert_eq!(
            position.piece_at(Square::at(4, 0)),
            Some(Piece::new(Color::White, PieceKind::King))
        );
        assert_eq!(
            position.piece_at(Square::at(3, 7)),
            Some(Piece::new(Color::Black, PieceKind::Queen))
        );
        assert_eq!(position.piece_at(Square::at(4, 4)), None);
        assert_eq!(position.board.iter().flatten().count(), 32);
    }

    #[test]
    fn fen_roundtrip() {
        for fen in [
            START_FEN,
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            "rnbqkbnr/pppp1ppp/8/4p3/4P3/8/PPPP1PPP/RNBQKBNR w KQkq e6 0 2",
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 12 40",
            "4k3/8/8/8/8/8/8/4K3 b - - 0 1",
        ] {
            assert_eq!(Position::from_fen(fen).unwrap().to_fen(), fen);
        }
    }

    #[test]
    fn fen_counters_are_optional() {
        let position = Position::from_fen("4k3/8/8/8/8/8/8/4K3 w - -").unwrap();
        assert_eq!(position.halfmove_clock, 0);
        assert_eq!(position.fullmove_number, 1);
    }

    #[test]
    fn fen_rejects_garbage() {
        for fen in [
            "",
            "not a fen",
            "8/8/8/8/8/8/8/8 w - - 0 1",      // нет королей
            "4k3/8/8/8/8/8/8/4K3 x - - 0 1",  // чей ход
            "4k3/8/8/8/8/8/8/4K3 w KQ - 0 1", // права без ладей
            "4k3/8/8/8/8/8/8/4K3 w - e3 0 1", // ep без пешки
            "4k3/8/8/8/8/8/8/4K3 w - - a 1",  // счётчик
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBN w KQkq - 0 1", // 7 клеток
            "rnbqkbnr/pppppppp/9/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", // цифра 9
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1", // 7 рангов
            "4k2P/8/8/8/8/8/8/4K3 w - - 0 1", // пешка на 8-м ранге
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KKkq - 0 1", // дубль права
            "88888888888888888888888888888888888888888/8/8/8/8/8/8/8 w - - 0 1",
        ] {
            assert!(Position::from_fen(fen).is_err(), "{fen:?}");
        }
    }

    #[test]
    fn king_square_is_found() {
        let position = Position::startpos();
        assert_eq!(position.king_square(Color::White), Some(Square::at(4, 0)));
        assert_eq!(position.king_square(Color::Black), Some(Square::at(4, 7)));
    }
}
