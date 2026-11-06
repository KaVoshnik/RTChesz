import { ApiError, getToken, clearToken } from '../api';

const API_URL: string = import.meta.env.VITE_API_URL ?? 'http://127.0.0.1:3000';
const TIMEOUT_MS = 10_000;

export type Side = 'white' | 'black';
export type GameStatus = 'waiting' | 'active' | 'finished' | 'aborted';

export interface Player {
  id: string;
  username: string;
}

export interface TimeControl {
  initialSecs: number;
  incrementSecs: number;
}

export interface GameSummary {
  id: string;
  status: GameStatus;
  white: Player | null;
  black: Player | null;
  timeControl: TimeControl;
  ply: number;
  result: Side | 'draw' | null;
  termination: string | null;
  createdAt: string;
}

export interface Game extends GameSummary {
  ruleset: string;
  moves: string[];
  fen: string;
  turn: Side | null;
  clock: { whiteMs: number; blackMs: number; running: Side | null };
  drawOfferedBy: Side | null;
  serverTime: string;
  startedAt: string | null;
  finishedAt: string | null;
}

export interface Me {
  id: string;
  username: string;
  email: string;
}

async function request<T>(method: 'GET' | 'POST', path: string, body?: unknown): Promise<T> {
  const token = getToken();
  let res: Response;
  try {
    res = await fetch(`${API_URL}${path}`, {
      method,
      headers: {
        ...(body !== undefined ? { 'Content-Type': 'application/json' } : {}),
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: body !== undefined ? JSON.stringify(body) : undefined,
      signal: AbortSignal.timeout(TIMEOUT_MS),
    });
  } catch {
    throw new ApiError('Не удалось связаться с сервером', 0);
  }

  const data = await res.json().catch(() => null);
  if (res.status === 401 && token) clearToken(); // токен отозван или просрочен
  if (!res.ok) {
    const retry = Number(res.headers.get('Retry-After')) || undefined;
    throw new ApiError(
      data?.message ?? 'Что-то пошло не так',
      res.status,
      res.status === 429 ? retry : undefined
    );
  }
  return data as T;
}

export const me = () => request<Me>('GET', '/auth/me');
export const lobby = () => request<{ games: GameSummary[] }>('GET', '/games/lobby');
export const mine = () => request<{ games: GameSummary[] }>('GET', '/games/mine');
export const getGame = (id: string) => request<Game>('GET', `/games/${id}`);

export const createGame = (timeControl: TimeControl, color: Side | 'random') =>
  request<Game>('POST', '/games', { timeControl, color });
export const joinGame = (id: string) => request<Game>('POST', `/games/${id}/join`, {});
export const sendMove = (id: string, move: string) =>
  request<Game>('POST', `/games/${id}/moves`, { move });
export const resign = (id: string) => request<Game>('POST', `/games/${id}/resign`, {});
export const offerDraw = (id: string) => request<Game>('POST', `/games/${id}/draw`, {});
export const declineDraw = (id: string) => request<Game>('POST', `/games/${id}/draw/decline`, {});
// Для своего вызова (waiting) сервер отвечает { message }, для идущей партии - GameResponse
export const abortGame = (id: string) =>
  request<Game | { message: string }>('POST', `/games/${id}/abort`, {});
