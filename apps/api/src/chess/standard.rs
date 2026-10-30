//! Обычные шахматы по правилам FIDE.
//!
//! Функции ниже сделаны публичными и независимыми от [`StandardRules`]: набор правил с
//! изменениями может собрать свой `legal_moves`/`apply` из тех же кирпичиков.

use super::{
    position::{Position, PositionKey},
    rules::{Outcome, Ruleset, STANDARD, Termination},
    types::{Color, Move, Piece, PieceKind, Square},
};

const KNIGHT_OFFSETS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING_OFFSETS: [(i8, i8); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];
const BISHOP_DIRECTIONS: [(i8, i8); 4] = [(1, 1), (1, -1), (-1, 1), (-1, -1)];
const ROOK_DIRECTIONS: [(i8, i8); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
const PROMOTION_KINDS: [PieceKind; 4] = [
    PieceKind::Queen,
    PieceKind::Rook,
    PieceKind::Bishop,
    PieceKind::Knight,
];

/// Обычные шахматы. Ничьи по 50 ходам и троекратному повторению здесь *автоматические*
/// (в настоящих шахматах их нужно требовать): так партии на сервере не зависают.
/// Пороги можно поменять в полях.
#[derive(Clone, Debug)]
pub struct StandardRules {
    /// Сколько полуходов без взятий и ходов пешек даёт ничью (100 = правило 50 ходов).
    pub draw_halfmove_limit: u32,
    /// Сколько раз должна повториться позиция для ничьей.
    pub repetition_limit: usize,
}

impl Default for StandardRules {
    fn default() -> Self {
        Self {
            draw_halfmove_limit: 100,
            repetition_limit: 3,
        }
    }
}

impl Ruleset for StandardRules {
    fn id(&self) -> &'static str {
        STANDARD
    }

    fn initial_position(&self) -> Position {
        Position::startpos()
    }

    fn legal_moves(&self, position: &Position) -> Vec<Move> {
        legal_moves(position)
    }

    fn apply(&self, position: &Position, mv: Move) -> Position {
        apply_move(position, mv)
    }

    fn position_key(&self, position: &Position) -> PositionKey {
        // Взятие на проходе входит в ключ, только если его реально можно сыграть:
        // иначе позиции, которые FIDE считает одинаковыми, имели бы разные ключи.
        let en_passant = position.en_passant.filter(|&target| {
            legal_moves(position).iter().any(|mv| {
                mv.to == target
                    && matches!(position.piece_at(mv.from), Some(p) if p.kind == PieceKind::Pawn)
            })
        });

        PositionKey::new(position, en_passant)
    }

    fn outcome(&self, position: &Position, history: &[PositionKey]) -> Option<Outcome> {
        if legal_moves(position).is_empty() {
            return Some(if is_in_check(position, position.turn) {
                Outcome::win(position.turn.opposite(), Termination::Checkmate)
            } else {
                Outcome::draw(Termination::Stalemate)
            });
        }

        if has_insufficient_material(position) {
            return Some(Outcome::draw(Termination::InsufficientMaterial));
        }

        if position.halfmove_clock >= self.draw_halfmove_limit {
            return Some(Outcome::draw(Termination::FiftyMoveRule));
        }

        let key = self.position_key(position);
        let repeats = history.iter().filter(|seen| **seen == key).count();
        if repeats >= self.repetition_limit {
            return Some(Outcome::draw(Termination::ThreefoldRepetition));
        }

        None
    }
}

/// Полностью разрешённые ходы: псевдолегальные без тех, после которых свой король под боем.
pub fn legal_moves(position: &Position) -> Vec<Move> {
    let mover = position.turn;

    pseudo_legal_moves(position)
        .into_iter()
        .filter(|&mv| !is_in_check(&apply_move(position, mv), mover))
        .collect()
}

/// Ходы по правилам движения фигур, без проверки, что свой король остаётся в безопасности
/// (рокировка здесь уже учитывает шах и бьющиеся поля, их проверять отдельно не надо).
pub fn pseudo_legal_moves(position: &Position) -> Vec<Move> {
    let us = position.turn;
    let mut moves = Vec::with_capacity(48);

    for from in Square::all() {
        let Some(piece) = position.piece_at(from) else {
            continue;
        };
        if piece.color != us {
            continue;
        }

        match piece.kind {
            PieceKind::Pawn => pawn_moves(position, from, us, &mut moves),
            PieceKind::Knight => step_moves(position, from, us, &KNIGHT_OFFSETS, &mut moves),
            PieceKind::Bishop => slide_moves(position, from, us, &BISHOP_DIRECTIONS, &mut moves),
            PieceKind::Rook => slide_moves(position, from, us, &ROOK_DIRECTIONS, &mut moves),
            PieceKind::Queen => {
                slide_moves(position, from, us, &BISHOP_DIRECTIONS, &mut moves);
                slide_moves(position, from, us, &ROOK_DIRECTIONS, &mut moves);
            }
            PieceKind::King => {
                step_moves(position, from, us, &KING_OFFSETS, &mut moves);
                castling_moves(position, from, us, &mut moves);
            }
        }
    }

    moves
}

/// Ходы "шагом" (конь, король): на любую клетку со смещением, если там нет своей фигуры.
fn step_moves(
    position: &Position,
    from: Square,
    us: Color,
    offsets: &[(i8, i8)],
    moves: &mut Vec<Move>,
) {
    for &(df, dr) in offsets {
        let Some(to) = from.offset(df, dr) else {
            continue;
        };
        match position.piece_at(to) {
            Some(piece) if piece.color == us => {}
            _ => moves.push(Move::new(from, to)),
        }
    }
}

/// Ходы "лучом" (слон, ладья, ферзь): до первой фигуры; чужую можно взять.
fn slide_moves(
    position: &Position,
    from: Square,
    us: Color,
    directions: &[(i8, i8)],
    moves: &mut Vec<Move>,
) {
    for &(df, dr) in directions {
        let mut current = from;
        while let Some(to) = current.offset(df, dr) {
            match position.piece_at(to) {
                None => {
                    moves.push(Move::new(from, to));
                    current = to;
                }
                Some(piece) => {
                    if piece.color != us {
                        moves.push(Move::new(from, to));
                    }
                    break;
                }
            }
        }
    }
}

fn pawn_moves(position: &Position, from: Square, us: Color, moves: &mut Vec<Move>) {
    let direction = us.pawn_direction();

    if let Some(one) = from.offset(0, direction)
        && position.piece_at(one).is_none()
    {
        push_pawn_move(from, one, us, moves);

        if from.rank() == us.pawn_start_rank()
            && let Some(two) = from.offset(0, 2 * direction)
            && position.piece_at(two).is_none()
        {
            moves.push(Move::new(from, two));
        }
    }

    for df in [-1, 1] {
        let Some(to) = from.offset(df, direction) else {
            continue;
        };
        match position.piece_at(to) {
            Some(target) if target.color != us => push_pawn_move(from, to, us, moves),
            None if position.en_passant == Some(to) => moves.push(Move::new(from, to)),
            _ => {}
        }
    }
}

/// Ход пешки; на последнем ранге разворачивается в четыре варианта превращения.
fn push_pawn_move(from: Square, to: Square, us: Color, moves: &mut Vec<Move>) {
    if to.rank() == us.promotion_rank() {
        for kind in PROMOTION_KINDS {
            moves.push(Move::with_promotion(from, to, kind));
        }
    } else {
        moves.push(Move::new(from, to));
    }
}

/// Рокировка (только классическая, король с e-вертикали). Условия: право не потеряно,
/// ладья на месте, поля между королём и ладьёй пусты, король не под шахом и не проходит
/// через бьющиеся поля.
fn castling_moves(position: &Position, from: Square, us: Color, moves: &mut Vec<Move>) {
    let rank = us.home_rank();
    if from != Square::at(4, rank) {
        return;
    }

    let enemy = us.opposite();
    let rook = Piece::new(us, PieceKind::Rook);
    let empty = |file: u8| position.piece_at(Square::at(file, rank)).is_none();
    let safe = |file: u8| !is_square_attacked(position, Square::at(file, rank), enemy);

    if !(position.castling.kingside(us) || position.castling.queenside(us))
        || is_square_attacked(position, from, enemy)
    {
        return;
    }

    if position.castling.kingside(us)
        && position.piece_at(Square::at(7, rank)) == Some(rook)
        && empty(5)
        && empty(6)
        && safe(5)
        && safe(6)
    {
        moves.push(Move::new(from, Square::at(6, rank)));
    }

    if position.castling.queenside(us)
        && position.piece_at(Square::at(0, rank)) == Some(rook)
        && empty(1)
        && empty(2)
        && empty(3)
        && safe(3)
        && safe(2)
    {
        moves.push(Move::new(from, Square::at(2, rank)));
    }
}

/// Бьёт ли сторона `by` клетку `square` (независимо от того, что на ней стоит).
pub fn is_square_attacked(position: &Position, square: Square, by: Color) -> bool {
    let piece_is =
        |sq: Square, kind: PieceKind| position.piece_at(sq) == Some(Piece::new(by, kind));

    // Пешка `by` бьёт вперёд по диагонали, значит, атакующая стоит на ранг "позади".
    for df in [-1, 1] {
        if let Some(sq) = square.offset(df, -by.pawn_direction())
            && piece_is(sq, PieceKind::Pawn)
        {
            return true;
        }
    }

    for (df, dr) in KNIGHT_OFFSETS {
        if let Some(sq) = square.offset(df, dr)
            && piece_is(sq, PieceKind::Knight)
        {
            return true;
        }
    }

    for (df, dr) in KING_OFFSETS {
        if let Some(sq) = square.offset(df, dr)
            && piece_is(sq, PieceKind::King)
        {
            return true;
        }
    }

    ray_attacked(position, square, by, &BISHOP_DIRECTIONS, PieceKind::Bishop)
        || ray_attacked(position, square, by, &ROOK_DIRECTIONS, PieceKind::Rook)
}

/// Первая фигура на луче - слон/ладья (в зависимости от направлений) или ферзь стороны `by`.
fn ray_attacked(
    position: &Position,
    square: Square,
    by: Color,
    directions: &[(i8, i8)],
    slider: PieceKind,
) -> bool {
    for &(df, dr) in directions {
        let mut current = square;
        while let Some(next) = current.offset(df, dr) {
            if let Some(piece) = position.piece_at(next) {
                if piece.color == by && (piece.kind == slider || piece.kind == PieceKind::Queen) {
                    return true;
                }
                break;
            }
            current = next;
        }
    }

    false
}

/// Стоит ли король цвета `color` под боем. Без короля - `false`.
pub fn is_in_check(position: &Position, color: Color) -> bool {
    position
        .king_square(color)
        .is_some_and(|king| is_square_attacked(position, king, color.opposite()))
}

/// Применяет ход к копии позиции: двигает фигуру, снимает побитое (в том числе при взятии
/// на проходе), переставляет ладью при рокировке, превращает пешку, обновляет права на
/// рокировку, цель взятия на проходе, счётчики и очередь хода.
///
/// Ход должен быть легальным в `position` (в `Game` это проверяется заранее).
pub fn apply_move(position: &Position, mv: Move) -> Position {
    let mut next = position.clone();

    let Some(piece) = position.piece_at(mv.from) else {
        debug_assert!(false, "no piece on the origin square of {mv}");
        return next;
    };

    let mut is_capture = position.piece_at(mv.to).is_some();

    // Взятие на проходе: пешка уходит по диагонали на пустую клетку, побитая стоит рядом.
    if piece.kind == PieceKind::Pawn
        && !is_capture
        && position.en_passant == Some(mv.to)
        && mv.from.file() != mv.to.file()
    {
        next.set_piece(Square::at(mv.to.file(), mv.from.rank()), None);
        is_capture = true;
    }

    // Рокировка: король шагнул на две клетки, ладья перепрыгивает через него.
    if piece.kind == PieceKind::King && mv.from.file().abs_diff(mv.to.file()) == 2 {
        let rank = mv.from.rank();
        let (rook_from, rook_to) = if mv.to.file() == 6 { (7, 5) } else { (0, 3) };
        let rook = next.piece_at(Square::at(rook_from, rank));
        next.set_piece(Square::at(rook_from, rank), None);
        next.set_piece(Square::at(rook_to, rank), rook);
    }

    let placed = match mv.promotion {
        Some(kind) if piece.kind == PieceKind::Pawn => Piece::new(piece.color, kind),
        _ => piece,
    };
    next.set_piece(mv.from, None);
    next.set_piece(mv.to, Some(placed));

    // Права на рокировку теряются при ходе короля и при любом касании угла доски
    // (ходит ладья или её берут).
    if piece.kind == PieceKind::King {
        next.castling.clear_color(piece.color);
    }
    for square in [mv.from, mv.to] {
        match (square.file(), square.rank()) {
            (0, 0) => next.castling.white_queenside = false,
            (7, 0) => next.castling.white_kingside = false,
            (0, 7) => next.castling.black_queenside = false,
            (7, 7) => next.castling.black_kingside = false,
            _ => {}
        }
    }

    next.en_passant = if piece.kind == PieceKind::Pawn && mv.from.rank().abs_diff(mv.to.rank()) == 2
    {
        Some(Square::at(
            mv.from.file(),
            u8::midpoint(mv.from.rank(), mv.to.rank()),
        ))
    } else {
        None
    };

    next.halfmove_clock = if piece.kind == PieceKind::Pawn || is_capture {
        0
    } else {
        position.halfmove_clock + 1
    };
    if position.turn == Color::Black {
        next.fullmove_number += 1;
    }
    next.turn = position.turn.opposite();

    next
}

/// Мёртвая позиция: ни у одной стороны нет материала для мата ни при каких ходах.
/// Король против короля, король + конь/слон против короля, слоны только одного цвета полей.
pub fn has_insufficient_material(position: &Position) -> bool {
    let mut knights = 0;
    let mut light_bishops = false;
    let mut dark_bishops = false;

    for square in Square::all() {
        let Some(piece) = position.piece_at(square) else {
            continue;
        };
        match piece.kind {
            PieceKind::King => {}
            PieceKind::Pawn | PieceKind::Rook | PieceKind::Queen => return false,
            PieceKind::Knight => knights += 1,
            PieceKind::Bishop => {
                if (square.file() + square.rank()) % 2 == 0 {
                    dark_bishops = true;
                } else {
                    light_bishops = true;
                }
            }
        }
    }

    match (knights, light_bishops, dark_bishops) {
        (0, light, dark) => !(light && dark),
        (1, false, false) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chess::rules::perft;

    fn fen(text: &str) -> Position {
        Position::from_fen(text).expect("test FEN is valid")
    }

    fn uci(text: &str) -> Move {
        Move::from_uci(text).expect("test move is valid")
    }

    fn legal_uci(position: &Position) -> Vec<String> {
        let mut moves: Vec<String> = legal_moves(position).iter().map(Move::to_string).collect();
        moves.sort();
        moves
    }

    fn assert_perft(text: &str, expected: &[u64]) {
        let rules = StandardRules::default();
        let position = fen(text);

        for (depth, &nodes) in (1u32..).zip(expected) {
            assert_eq!(
                perft(&rules, &position, depth),
                nodes,
                "{text} depth {depth}"
            );
        }
    }

    // Эталонные числа с chessprogramming.org/Perft_Results.

    #[test]
    fn perft_start_position() {
        assert_perft(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            &[20, 400, 8_902, 197_281],
        );
    }

    #[test]
    fn perft_kiwipete() {
        assert_perft(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            &[48, 2_039, 97_862],
        );
    }

    #[test]
    fn perft_endgame_with_en_passant_pins() {
        assert_perft(
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            &[14, 191, 2_812, 43_238],
        );
    }

    #[test]
    fn perft_promotions_and_castling_rights() {
        assert_perft(
            "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1",
            &[6, 264, 9_467],
        );
        // То же с переставленными цветами: ловит ошибки, зависящие от цвета.
        assert_perft(
            "r2q1rk1/pP1p2pp/Q4n2/bbp1p3/Np6/1B3NBn/pPPP1PPP/R3K2R b KQ - 0 1",
            &[6, 264, 9_467],
        );
    }

    #[test]
    fn perft_position_five() {
        assert_perft(
            "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8",
            &[44, 1_486, 62_379],
        );
    }

    #[test]
    fn perft_position_six() {
        assert_perft(
            "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
            &[46, 2_079, 89_890],
        );
    }

    /// Долгие проверки глубже: `cargo test -- --ignored`.
    #[test]
    #[ignore = "медленно в debug-сборке"]
    fn perft_deep() {
        assert_perft(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            &[20, 400, 8_902, 197_281, 4_865_609],
        );
        assert_perft(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
            &[48, 2_039, 97_862, 4_085_603],
        );
        assert_perft(
            "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
            &[14, 191, 2_812, 43_238, 674_624],
        );
    }

    #[test]
    fn castling_is_generated_and_applied() {
        let position = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        let moves = legal_uci(&position);
        assert!(moves.contains(&"e1g1".to_string()));
        assert!(moves.contains(&"e1c1".to_string()));

        let after = apply_move(&position, uci("e1g1"));
        assert_eq!(after.to_fen(), "r3k2r/8/8/8/8/8/8/R4RK1 b kq - 1 1");

        let after = apply_move(&position, uci("e1c1"));
        assert_eq!(after.to_fen(), "r3k2r/8/8/8/8/8/8/2KR3R b kq - 1 1");
    }

    #[test]
    fn castling_is_forbidden_out_of_through_and_after_losing_rights() {
        // Шах королю: рокировки нет.
        let in_check = fen("r3k2r/8/8/8/8/8/4r3/R3K2R w KQkq - 0 1");
        let moves = legal_uci(&in_check);
        assert!(!moves.contains(&"e1g1".to_string()));
        assert!(!moves.contains(&"e1c1".to_string()));

        // Поле f1 под боем: короткая нельзя, длинная можно.
        let through = fen("r3k2r/8/8/8/8/8/5r2/R3K2R w KQkq - 0 1");
        let moves = legal_uci(&through);
        assert!(!moves.contains(&"e1g1".to_string()));
        assert!(moves.contains(&"e1c1".to_string()));

        // Поле b1 может быть под боем: ладья его проходит, король - нет.
        let rook_passes = fen("r3k2r/8/8/8/8/8/1r6/R3K2R w KQkq - 0 1");
        assert!(legal_uci(&rook_passes).contains(&"e1c1".to_string()));

        // Между королём и ладьёй стоит фигура.
        let blocked = fen("r3k2r/8/8/8/8/8/8/RN2K2R w KQkq - 0 1");
        assert!(!legal_uci(&blocked).contains(&"e1c1".to_string()));

        // Ладья сходила и вернулась - право потеряно.
        let mut position = fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1");
        for text in ["h1h2", "a8a7", "h2h1", "a7a8"] {
            position = apply_move(&position, uci(text));
        }
        assert!(!position.castling.white_kingside);
        assert!(!legal_uci(&position).contains(&"e1g1".to_string()));
        assert!(position.castling.white_queenside);
    }

    #[test]
    fn capturing_a_rook_removes_castling_right() {
        let position = fen("r3k2r/8/8/8/8/8/6b1/R3K2R b KQkq - 0 1");
        let after = apply_move(&position, uci("g2h1"));
        assert!(!after.castling.white_kingside);
        assert!(after.castling.white_queenside);
    }

    #[test]
    fn en_passant_capture() {
        let mut position = Position::startpos();
        for text in ["e2e4", "a7a6", "e4e5", "d7d5"] {
            position = apply_move(&position, uci(text));
        }
        assert_eq!(position.en_passant, Square::parse("d6"));
        assert!(legal_uci(&position).contains(&"e5d6".to_string()));

        let after = apply_move(&position, uci("e5d6"));
        assert_eq!(after.piece_at(Square::parse("d5").unwrap()), None);
        assert_eq!(
            after.piece_at(Square::parse("d6").unwrap()),
            Some(Piece::new(Color::White, PieceKind::Pawn))
        );
        assert_eq!(after.en_passant, None);
        assert_eq!(after.halfmove_clock, 0);
    }

    #[test]
    fn en_passant_that_exposes_the_king_is_illegal() {
        // Чёрная пешка e4 бьёт d3 на проходе, но тогда ферзь h4 берёт короля a4 по рангу.
        let position = fen("8/8/8/8/k2Pp2Q/8/8/3K4 b - d3 0 1");
        let moves = legal_uci(&position);

        assert!(!moves.contains(&"e4d3".to_string()));
        assert!(moves.contains(&"e4e3".to_string()));
    }

    #[test]
    fn promotion_offers_four_pieces_and_replaces_the_pawn() {
        let position = fen("8/P7/8/8/8/8/8/k6K w - - 0 1");
        let moves = legal_uci(&position);

        for promotion in ["a7a8q", "a7a8r", "a7a8b", "a7a8n"] {
            assert!(moves.contains(&promotion.to_string()), "{promotion}");
        }
        assert!(!moves.contains(&"a7a8".to_string()));

        let after = apply_move(&position, uci("a7a8n"));
        assert_eq!(
            after.piece_at(Square::parse("a8").unwrap()),
            Some(Piece::new(Color::White, PieceKind::Knight))
        );
    }

    #[test]
    fn pinned_piece_cannot_leave_the_line() {
        // Слон e2 связан ладьёй e8 с королём e1.
        let position = fen("4r2k/8/8/8/8/8/4B3/4K3 w - - 0 1");
        assert!(legal_uci(&position).iter().all(|mv| !mv.starts_with("e2")));
    }

    #[test]
    fn in_check_only_king_moves_or_blocks() {
        // Шах ладьёй e8: король уходит с вертикали или ферзь закрывает.
        let position = fen("4r2k/8/8/8/8/8/8/3QK3 w - - 0 1");
        let moves = legal_uci(&position);

        assert!(moves.contains(&"d1e2".to_string()));
        assert!(moves.contains(&"e1d2".to_string()));
        assert!(!moves.contains(&"d1d2".to_string()));
        assert!(!moves.contains(&"e1e2".to_string()));
    }

    #[test]
    fn halfmove_and_fullmove_counters() {
        let position = Position::startpos();

        let after = apply_move(&position, uci("g1f3"));
        assert_eq!((after.halfmove_clock, after.fullmove_number), (1, 1));

        let after = apply_move(&after, uci("g8f6"));
        assert_eq!((after.halfmove_clock, after.fullmove_number), (2, 2));

        let after = apply_move(&after, uci("e2e4"));
        assert_eq!(after.halfmove_clock, 0);
    }

    #[test]
    fn insufficient_material_cases() {
        for dead in [
            "4k3/8/8/8/8/8/8/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/3NK3 w - - 0 1",
            "4k3/8/8/8/8/8/8/3BK3 w - - 0 1",
            "2b1k3/8/8/8/8/8/8/3BK3 w - - 0 1", // слоны на полях одного цвета: d1 и c8
        ] {
            assert!(has_insufficient_material(&fen(dead)), "{dead}");
        }

        for alive in [
            "4k3/8/8/8/8/8/8/3RK3 w - - 0 1",
            "4k3/8/8/8/8/8/4P3/4K3 w - - 0 1",
            "4k3/8/8/8/8/8/8/2NNK3 w - - 0 1",
            "4kb2/8/8/8/8/8/8/4KB2 w - - 0 1", // слоны на полях разного цвета: f1 и f8
            "4k3/8/8/8/8/8/8/2BNK3 w - - 0 1",
        ] {
            assert!(!has_insufficient_material(&fen(alive)), "{alive}");
        }
    }

    #[test]
    fn outcome_checkmate_stalemate_and_fifty_moves() {
        let rules = StandardRules::default();
        let outcome = |text: &str| {
            let position = fen(text);
            rules.outcome(&position, &[rules.position_key(&position)])
        };

        assert_eq!(
            outcome("7k/6Q1/6K1/8/8/8/8/8 b - - 0 1"),
            Some(Outcome::win(Color::White, Termination::Checkmate))
        );
        assert_eq!(
            outcome("7k/5Q2/6K1/8/8/8/8/8 b - - 0 1"),
            Some(Outcome::draw(Termination::Stalemate))
        );
        assert_eq!(
            outcome("4k3/8/8/8/8/8/3R4/4K3 w - - 100 80"),
            Some(Outcome::draw(Termination::FiftyMoveRule))
        );
        assert_eq!(outcome("4k3/8/8/8/8/8/3R4/4K3 w - - 99 80"), None);
        assert_eq!(
            outcome("4k3/8/8/8/8/8/8/4K3 w - - 0 1"),
            Some(Outcome::draw(Termination::InsufficientMaterial))
        );
    }

    #[test]
    fn mate_beats_fifty_move_rule() {
        let rules = StandardRules::default();
        let position = fen("7k/6Q1/6K1/8/8/8/8/8 b - - 100 90");
        let key = rules.position_key(&position);

        assert_eq!(
            rules.outcome(&position, &[key]),
            Some(Outcome::win(Color::White, Termination::Checkmate))
        );
    }

    #[test]
    fn repetition_ignores_unplayable_en_passant() {
        let rules = StandardRules::default();

        // Цель e3 после e2e4 нигде не может быть взята: ключ такой же, как без неё.
        let mut position = Position::startpos();
        position = apply_move(&position, uci("e2e4"));
        assert!(position.en_passant.is_some());

        let mut without = position.clone();
        without.en_passant = None;
        assert_eq!(rules.position_key(&position), rules.position_key(&without));
    }
}
