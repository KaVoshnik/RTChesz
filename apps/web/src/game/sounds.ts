// Звуки партии. Файлы лежат в src/assets/sounds, Vite сам подставит им правильные адреса.
import checkUrl from '../assets/sounds/check_sound.wav';
import defeatUrl from '../assets/sounds/defeat_sound.wav';
import middleUrl from '../assets/sounds/middle_hit.wav';
import moveUrl from '../assets/sounds/move_sound.wav';
import pawnUrl from '../assets/sounds/pawn_hit.wav';
import queenUrl from '../assets/sounds/queen_hit.wav';
import startUrl from '../assets/sounds/start_sound.wav';
import { kingInCheck, uciToMv, type Pos } from './chess';

export type SoundName = 'start' | 'move' | 'pawn' | 'middle' | 'queen' | 'check' | 'defeat';

const URLS: Record<SoundName, string> = {
  start: startUrl,
  move: moveUrl,
  pawn: pawnUrl,
  middle: middleUrl,
  queen: queenUrl,
  check: checkUrl,
  defeat: defeatUrl,
};

const VOLUME = 0.7; // от 0 до 1

// Загружаем один раз, чтобы звук не «запаздывал» при первом ходе
const cache = new Map<SoundName, HTMLAudioElement>();
function audio(name: SoundName): HTMLAudioElement {
  let a = cache.get(name);
  if (!a) {
    a = new Audio(URLS[name]);
    a.preload = 'auto';
    cache.set(name, a);
  }
  return a;
}

export function playSound(name: SoundName) {
  try {
    // cloneNode: если ходы идут подряд, звуки не обрывают друг друга
    const a = audio(name).cloneNode() as HTMLAudioElement;
    a.volume = VOLUME;
    // Браузер может запретить звук, пока человек не кликнул по странице - тогда молчим
    a.play().catch(() => {});
  } catch {
    // звук - не главное, игре он мешать не должен
  }
}

/**
 * Какой звук у последнего хода.
 * positions[i] - позиция ДО i-го хода, positions[moves.length] - позиция после последнего.
 * Приоритет: шах > взятие > обычный ход.
 */
export function soundForLastMove(positions: Pos[], moves: string[]): SoundName {
  const i = moves.length - 1;
  const before = positions[i];
  const after = positions[i + 1];
  if (!before || !after) return 'move';

  const m = uciToMv(moves[i]);
  const piece = before.board[m.from];
  let victim = before.board[m.to];
  if (
    !victim &&
    piece &&
    piece.toLowerCase() === 'p' &&
    m.to === before.ep &&
    m.from % 8 !== m.to % 8
  )
    victim = 'p'; // взятие на проходе

  // Шах: король того, кто теперь ходит, под боем
  if (kingInCheck(after) >= 0) return 'check';

  if (!victim) return 'move';
  // Звук зависит от срубленной фигуры: пешка / ладья-конь-слон / ферзь
  const kind = victim.toLowerCase();
  if (kind === 'p') return 'pawn';
  if (kind === 'q') return 'queen';
  return 'middle';
}
