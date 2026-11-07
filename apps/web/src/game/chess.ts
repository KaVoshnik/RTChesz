// Лёгкий генератор ходов на клиенте. Нужен ТОЛЬКО для подсказок (точки на клетках,
// выбор превращения пешки). Истина - на сервере: он сам проверяет каждый ход.

export type Color = 'w' | 'b';

export interface Pos {
  board: (string | null)[]; // 64 клетки, индекс 0 = a8, 63 = h1
  turn: Color;
  castling: string;
  ep: number; // клетка взятия на проходе или -1
}

export interface Mv {
  from: number;
  to: number;
  promo?: 'q' | 'r' | 'b' | 'n';
}

const N = [
  [-2, -1],
  [-2, 1],
  [-1, -2],
  [-1, 2],
  [1, -2],
  [1, 2],
  [2, -1],
  [2, 1],
];
const K = [
  [-1, -1],
  [-1, 0],
  [-1, 1],
  [0, -1],
  [0, 1],
  [1, -1],
  [1, 0],
  [1, 1],
];
const DIAG = [
  [-1, -1],
  [-1, 1],
  [1, -1],
  [1, 1],
];
const ORTHO = [
  [-1, 0],
  [1, 0],
  [0, -1],
  [0, 1],
];

export const colorOf = (p: string): Color => (p === p.toUpperCase() ? 'w' : 'b');
export const sqName = (i: number) => String.fromCharCode(97 + (i % 8)) + (8 - Math.floor(i / 8));
export const sqIndex = (name: string) => (8 - Number(name[1])) * 8 + (name.charCodeAt(0) - 97);

export function parseFen(fen: string): Pos {
  const [placement, turn, castling, ep] = fen.split(' ');
  const board: (string | null)[] = [];
  for (const ch of placement) {
    if (ch === '/') continue;
    if (/\d/.test(ch)) for (let i = 0; i < Number(ch); i++) board.push(null);
    else board.push(ch);
  }
  return {
    board,
    turn: turn === 'b' ? 'b' : 'w',
    castling: castling && castling !== '-' ? castling : '',
    ep: ep && ep !== '-' ? sqIndex(ep) : -1,
  };
}

function attacked(board: (string | null)[], sq: number, by: Color): boolean {
  const r = Math.floor(sq / 8);
  const f = sq % 8;
  const at = (rr: number, ff: number) =>
    rr < 0 || rr > 7 || ff < 0 || ff > 7 ? null : board[rr * 8 + ff];
  const mine = (p: string | null, kinds: string) =>
    p !== null && colorOf(p) === by && kinds.includes(p.toLowerCase());

  const pr = by === 'w' ? r + 1 : r - 1;
  for (const df of [-1, 1]) if (mine(at(pr, f + df), 'p')) return true;
  for (const [dr, df] of N) if (mine(at(r + dr, f + df), 'n')) return true;
  for (const [dr, df] of K) if (mine(at(r + dr, f + df), 'k')) return true;
  for (const [dirs, kinds] of [
    [DIAG, 'bq'],
    [ORTHO, 'rq'],
  ] as const) {
    for (const [dr, df] of dirs) {
      let rr = r + dr;
      let ff = f + df;
      while (rr >= 0 && rr < 8 && ff >= 0 && ff < 8) {
        const p = board[rr * 8 + ff];
        if (p) {
          if (mine(p, kinds)) return true;
          break;
        }
        rr += dr;
        ff += df;
      }
    }
  }
  return false;
}

function pseudoMoves(pos: Pos): Mv[] {
  const { board, turn } = pos;
  const out: Mv[] = [];
  const enemy: Color = turn === 'w' ? 'b' : 'w';

  for (let from = 0; from < 64; from++) {
    const p = board[from];
    if (!p || colorOf(p) !== turn) continue;
    const r = Math.floor(from / 8);
    const f = from % 8;
    const kind = p.toLowerCase();

    if (kind === 'p') {
      const dir = turn === 'w' ? -1 : 1;
      const startRow = turn === 'w' ? 6 : 1;
      const lastRow = turn === 'w' ? 0 : 7;
      const push = (to: number) => {
        if (Math.floor(to / 8) === lastRow)
          for (const promo of ['q', 'r', 'b', 'n'] as const) out.push({ from, to, promo });
        else out.push({ from, to });
      };
      const one = (r + dir) * 8 + f;
      if (r + dir >= 0 && r + dir <= 7 && !board[one]) {
        push(one);
        const two = (r + 2 * dir) * 8 + f;
        if (r === startRow && !board[two]) out.push({ from, to: two });
      }
      for (const df of [-1, 1]) {
        const ff = f + df;
        const rr = r + dir;
        if (ff < 0 || ff > 7 || rr < 0 || rr > 7) continue;
        const to = rr * 8 + ff;
        const target = board[to];
        if ((target && colorOf(target) === enemy) || to === pos.ep) push(to);
      }
      continue;
    }

    const step = (dirs: number[][], slide: boolean) => {
      for (const [dr, df] of dirs) {
        let rr = r + dr;
        let ff = f + df;
        while (rr >= 0 && rr < 8 && ff >= 0 && ff < 8) {
          const to = rr * 8 + ff;
          const t = board[to];
          if (t && colorOf(t) === turn) break;
          out.push({ from, to });
          if (t || !slide) break;
          rr += dr;
          ff += df;
        }
      }
    };

    if (kind === 'n') step(N, false);
    else if (kind === 'b') step(DIAG, true);
    else if (kind === 'r') step(ORTHO, true);
    else if (kind === 'q') step([...DIAG, ...ORTHO], true);
    else if (kind === 'k') {
      step(K, false);
      const home = turn === 'w' ? 60 : 4;
      if (from === home && !attacked(board, home, enemy)) {
        const rook = turn === 'w' ? 'R' : 'r';
        const ks = turn === 'w' ? 'K' : 'k';
        const qs = turn === 'w' ? 'Q' : 'q';
        if (
          pos.castling.includes(ks) &&
          !board[home + 1] &&
          !board[home + 2] &&
          board[home + 3] === rook &&
          !attacked(board, home + 1, enemy)
        )
          out.push({ from, to: home + 2 });
        if (
          pos.castling.includes(qs) &&
          !board[home - 1] &&
          !board[home - 2] &&
          !board[home - 3] &&
          board[home - 4] === rook &&
          !attacked(board, home - 1, enemy)
        )
          out.push({ from, to: home - 2 });
      }
    }
  }
  return out;
}

function boardAfter(pos: Pos, m: Mv): (string | null)[] {
  const b = pos.board.slice();
  const p = b[m.from]!;
  const kind = p.toLowerCase();
  b[m.from] = null;
  if (kind === 'p' && m.to === pos.ep && m.from % 8 !== m.to % 8) {
    b[Math.floor(m.from / 8) * 8 + (m.to % 8)] = null; // взятие на проходе
  }
  if (kind === 'k' && Math.abs(m.to - m.from) === 2) {
    const rookFrom = m.to > m.from ? m.from + 3 : m.from - 4;
    const rookTo = m.to > m.from ? m.from + 1 : m.from - 1;
    b[rookTo] = b[rookFrom];
    b[rookFrom] = null;
  }
  b[m.to] = m.promo ? (colorOf(p) === 'w' ? m.promo.toUpperCase() : m.promo) : p;
  return b;
}

export function legalMoves(pos: Pos): Mv[] {
  const enemy: Color = pos.turn === 'w' ? 'b' : 'w';
  const king = pos.turn === 'w' ? 'K' : 'k';
  return pseudoMoves(pos).filter((m) => {
    const b = boardAfter(pos, m);
    return !attacked(b, b.indexOf(king), enemy);
  });
}

export function kingInCheck(pos: Pos): number {
  const king = pos.turn === 'w' ? 'K' : 'k';
  const sq = pos.board.indexOf(king);
  return sq >= 0 && attacked(pos.board, sq, pos.turn === 'w' ? 'b' : 'w') ? sq : -1;
}

export const moveToUci = (m: Mv) => sqName(m.from) + sqName(m.to) + (m.promo ?? '');

export const START_FEN = 'rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1';

export function uciToMv(uci: string): Mv {
  const promo = uci[4];
  return {
    from: sqIndex(uci.slice(0, 2)),
    to: sqIndex(uci.slice(2, 4)),
    ...(promo ? { promo: promo as NonNullable<Mv['promo']> } : {}),
  };
}

/** Делает ход (уже проверенный сервером) и возвращает новую позицию. */
export function applyMove(pos: Pos, m: Mv): Pos {
  const p = pos.board[m.from]!;
  const kind = p.toLowerCase();
  let castling = pos.castling;
  const strip = (rights: string) => {
    for (const c of rights) castling = castling.replace(c, '');
  };
  if (kind === 'k') strip(colorOf(p) === 'w' ? 'KQ' : 'kq');
  for (const sq of [m.from, m.to]) {
    if (sq === 63) strip('K');
    else if (sq === 56) strip('Q');
    else if (sq === 7) strip('k');
    else if (sq === 0) strip('q');
  }
  return {
    board: boardAfter(pos, m),
    turn: pos.turn === 'w' ? 'b' : 'w',
    castling,
    ep: kind === 'p' && Math.abs(m.to - m.from) === 16 ? (m.from + m.to) / 2 : -1,
  };
}

/** Все позиции партии: [i] - после i полуходов, [0] - начальная. */
export function replay(moves: string[]): Pos[] {
  const out = [parseFen(START_FEN)];
  for (const uci of moves) {
    const prev = out[out.length - 1];
    const m = uciToMv(uci);
    if (!prev.board[m.from]) break; // повреждённая запись: показываем сколько удалось
    out.push(applyMove(prev, m));
  }
  return out;
}

/** Фигуры, снятые с доски за первые `ply` полуходов (регистр буквы = цвет снятой фигуры). */
export function captured(positions: Pos[], moves: string[], ply: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < ply && i < positions.length - 1; i++) {
    const pos = positions[i];
    const m = uciToMv(moves[i]);
    const p = pos.board[m.from]!;
    let c = pos.board[m.to];
    if (!c && p.toLowerCase() === 'p' && m.to === pos.ep && m.from % 8 !== m.to % 8)
      c = pos.turn === 'w' ? 'p' : 'P'; // взятие на проходе
    if (c) out.push(c);
  }
  return out;
}
