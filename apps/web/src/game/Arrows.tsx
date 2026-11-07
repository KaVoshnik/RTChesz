// Стрелки и подсветки клеток, нарисованные правой кнопкой мыши (как на chess.com).
// Рисуем SVG поверх доски: 1 единица viewBox = 1 клетка.

export type Brush = 'green' | 'red' | 'blue' | 'yellow';
export interface Arrow {
  from: number;
  to: number;
  brush: Brush;
}
export interface Ring {
  sq: number;
  brush: Brush;
}

const COLORS: Record<Brush, string> = {
  green: '#2f9e44',
  red: '#d6382b',
  blue: '#2a6fdb',
  yellow: '#e0a800',
};

const WIDTH = 0.15;
const HEAD_LEN = 0.38;
const HEAD_HALF = 0.21;
const TIP_GAP = 0.1;

type P = { x: number; y: number };

const center = (sq: number, flipped: boolean): P => {
  const f = sq % 8;
  const r = Math.floor(sq / 8);
  return { x: (flipped ? 7 - f : f) + 0.5, y: (flipped ? 7 - r : r) + 0.5 };
};

function Shape({ arrow, flipped }: { arrow: Arrow; flipped: boolean }) {
  const a = center(arrow.from, flipped);
  const b = center(arrow.to, flipped);
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const ax = Math.abs(dx);
  const ay = Math.abs(dy);

  // Ход конём рисуем уголком: сначала вдоль длинной стороны, потом вдоль короткой
  const knight = (ax === 1 && ay === 2) || (ax === 2 && ay === 1);
  const pts: P[] = knight ? [a, ay === 2 ? { x: a.x, y: b.y } : { x: b.x, y: a.y }, b] : [a, b];

  const p = pts[pts.length - 2];
  const q = pts[pts.length - 1];
  const len = Math.hypot(q.x - p.x, q.y - p.y);
  const ux = (q.x - p.x) / len;
  const uy = (q.y - p.y) / len;
  const tip = { x: q.x - ux * TIP_GAP, y: q.y - uy * TIP_GAP };
  const base = { x: tip.x - ux * HEAD_LEN, y: tip.y - uy * HEAD_LEN };
  const body = [...pts.slice(0, -1), base].map((pt) => `${pt.x},${pt.y}`).join(' ');
  const head = [
    `${tip.x},${tip.y}`,
    `${base.x - uy * HEAD_HALF},${base.y + ux * HEAD_HALF}`,
    `${base.x + uy * HEAD_HALF},${base.y - ux * HEAD_HALF}`,
  ].join(' ');
  const color = COLORS[arrow.brush];

  return (
    <>
      <polyline
        points={body}
        fill="none"
        stroke={color}
        strokeWidth={WIDTH}
        strokeLinejoin="round"
      />
      <polygon points={head} fill={color} />
    </>
  );
}

function RingShape({ ring, flipped }: { ring: Ring; flipped: boolean }) {
  const c = center(ring.sq, flipped);
  return (
    <rect
      x={c.x - 0.5}
      y={c.y - 0.5}
      width={1}
      height={1}
      fill={COLORS[ring.brush]}
      opacity={0.5}
    />
  );
}

export function ArrowLayer({
  arrows,
  rings,
  preview,
  flipped,
}: {
  arrows: Arrow[];
  rings: Ring[];
  preview: Arrow | null; // то, что рисуется прямо сейчас, пока кнопка зажата
  flipped: boolean;
}) {
  return (
    <svg className="g-arrows" viewBox="0 0 8 8" aria-hidden="true">
      {rings.map((r) => (
        <RingShape key={r.sq} ring={r} flipped={flipped} />
      ))}
      <g opacity={0.8}>
        {arrows.map((a) => (
          <Shape key={`${a.from}-${a.to}`} arrow={a} flipped={flipped} />
        ))}
      </g>
      {preview &&
        (preview.from === preview.to ? (
          <RingShape ring={{ sq: preview.from, brush: preview.brush }} flipped={flipped} />
        ) : (
          <g opacity={0.5}>
            <Shape arrow={preview} flipped={flipped} />
          </g>
        ))}
    </svg>
  );
}
