// Timeline drawings: the drive card sparkline (inline SVG) and the trend chart (canvas). Colours come from theme variables.
import { DAY, size, tickLabel } from './fmt.js';

const SVG = 'http://www.w3.org/2000/svg';
const HOUR = 3600;
/** Tick steps, shortest first: count, unit, approximate seconds. */
const STEPS = [
  [1, 'h', HOUR], [3, 'h', 3 * HOUR], [6, 'h', 6 * HOUR], [12, 'h', 12 * HOUR],
  [1, 'd', DAY], [2, 'd', 2 * DAY], [7, 'd', 7 * DAY], [14, 'd', 14 * DAY],
  [1, 'm', 30 * DAY], [2, 'm', 61 * DAY], [3, 'm', 91 * DAY], [6, 'm', 182 * DAY], [12, 'y', 365 * DAY],
];
const TICK_PX = 80;
const PAD = { left: 8, right: 60, top: 10, bottom: 20 };

/** Change of `allocated` over the last `days`: the newest point against the newest one at least that old; null when the history is shorter. */
export function change(points, days) {
  const last = points.at(-1);
  if (!last) return null;
  const limit = last.time - days * DAY + DAY / 2;
  for (let i = points.length - 2; i >= 0; i--) {
    if (points[i].time <= limit) return last.allocated - points[i].allocated;
  }
  return null;
}

/** Polyline of used space over time in a 100x24 box, stretched to its element. */
export function sparkline(points) {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 100 24');
  svg.setAttribute('preserveAspectRatio', 'none');
  svg.classList.add('spark');
  const [t0, t1] = [points[0].time, points.at(-1).time];
  const values = points.map((p) => p.allocated);
  const [lo, hi] = [Math.min(...values), Math.max(...values)];
  const x = (t) => (t1 > t0 ? ((t - t0) / (t1 - t0)) * 100 : 50);
  const y = (v) => (hi > lo ? 22 - ((v - lo) / (hi - lo)) * 20 : 12);
  const line = document.createElementNS(SVG, 'polyline');
  line.setAttribute('points', points.map((p) => `${x(p.time).toFixed(2)},${y(p.allocated).toFixed(2)}`).join(' '));
  line.setAttribute('vector-effect', 'non-scaling-stroke');
  svg.append(line);
  return svg;
}

/** Start of the step containing `d` (local time), aligned to the unit. */
function align(d, n, unit) {
  if (unit === 'h') d.setHours(d.getHours() - (d.getHours() % n), 0, 0, 0);
  else d.setHours(0, 0, 0, 0);
  if (unit === 'd' && n === 7) d.setDate(d.getDate() - ((d.getDay() + 6) % 7));
  if (unit === 'm') d.setMonth(d.getMonth() - (d.getMonth() % n), 1);
  if (unit === 'y') d.setMonth(0, 1);
}

function advance(d, n, unit) {
  if (unit === 'h') d.setHours(d.getHours() + n);
  else if (unit === 'd') d.setDate(d.getDate() + n);
  else if (unit === 'm') d.setMonth(d.getMonth() + n);
  else d.setFullYear(d.getFullYear() + 1);
}

/** Tick times (Unix seconds) between `t0` and `t1` for an axis `width` pixels wide. */
export function timeTicks(t0, t1, width) {
  const max = Math.max(2, Math.floor(width / TICK_PX));
  const span = Math.max(t1 - t0, 1);
  const [n, unit] = STEPS.find(([, , secs]) => span / secs <= max) ?? STEPS.at(-1);
  const d = new Date(t0 * 1000);
  align(d, n, unit);
  const list = [];
  for (let guard = 0; d.getTime() / 1000 <= t1 && guard < 200; guard++) {
    const t = d.getTime() / 1000;
    if (t >= t0) list.push(t);
    advance(d, n, unit);
  }
  return { list, unit };
}

/** Rounds a range outward and returns about three gridline values. */
function sizeTicks(lo, hi) {
  if (hi <= lo) {
    const pad = Math.max(hi * 0.01, 1 << 20);
    [lo, hi] = [lo - pad, hi + pad];
  }
  const raw = (hi - lo) / 3;
  const mag = 1024 ** Math.max(0, Math.floor(Math.log(raw) / Math.log(1024)));
  const step = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000].map((m) => m * mag).find((s) => s >= raw) ?? raw;
  const start = Math.floor(lo / step) * step;
  const end = Math.ceil(hi / step) * step;
  const list = [];
  for (let v = start; v <= end + step / 2; v += step) list.push(v);
  return { lo: start, hi: end, list };
}

/** Theme colours read from the CSS variables. */
function colours() {
  const css = getComputedStyle(document.documentElement);
  const v = (name) => css.getPropertyValue(name).trim();
  return { line: v('--accent'), grid: v('--border'), text: v('--muted'), ink: v('--text'), bg: v('--surface'), font: v('--mono') };
}

/** Line chart of used space over a root's timeline with time ticks and a hover readout. */
export class Trend {
  /** `onHover(point, previous)` gets the hovered point, or the latest one when the pointer leaves. */
  constructor(canvas, onHover) {
    this.canvas = canvas;
    this.onHover = onHover;
    this.points = [];
    this.hover = -1;
    canvas.addEventListener('mousemove', (e) => this.pointer(e));
    canvas.addEventListener('mouseleave', () => this.setHover(-1));
    new ResizeObserver(() => this.draw()).observe(canvas);
    new MutationObserver(() => this.draw()).observe(document.documentElement, { attributeFilter: ['data-theme'] });
    addEventListener('resize', () => this.draw());
  }

  set(points) {
    this.points = points;
    this.hover = -1;
    this.draw();
    this.report();
  }

  report() {
    const i = this.hover >= 0 ? this.hover : this.points.length - 1;
    this.onHover(this.points[i] ?? null, this.points[i - 1] ?? null);
  }

  setHover(i) {
    if (i === this.hover) return;
    this.hover = i;
    this.draw();
    this.report();
  }

  /** Plot area and scales for the current size. */
  frame(w, h) {
    const p = this.points;
    const [t0, t1] = [p[0].time, p.at(-1).time];
    const y = sizeTicks(Math.min(...p.map((q) => q.allocated)), Math.max(...p.map((q) => q.allocated)));
    const [x0, x1, y0, y1] = [PAD.left, w - PAD.right, PAD.top, h - PAD.bottom];
    return {
      x0, x1, y0, y1, t0, t1, y,
      xOf: (t) => (t1 > t0 ? x0 + ((t - t0) / (t1 - t0)) * (x1 - x0) : (x0 + x1) / 2),
      yOf: (v) => y1 - ((v - y.lo) / (y.hi - y.lo)) * (y1 - y0),
    };
  }

  pointer(e) {
    if (!this.points.length) return;
    const f = this.frame(this.canvas.clientWidth, this.canvas.clientHeight);
    const mx = e.offsetX;
    let best = 0;
    this.points.forEach((p, i) => {
      if (Math.abs(f.xOf(p.time) - mx) < Math.abs(f.xOf(this.points[best].time) - mx)) best = i;
    });
    this.setHover(best);
  }

  draw() {
    const { canvas } = this;
    const dpr = devicePixelRatio || 1;
    const [w, h] = [canvas.clientWidth, canvas.clientHeight];
    if (!w || !h) return;
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    const ctx = canvas.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);
    if (!this.points.length) return;
    const c = colours();
    const f = this.frame(w, h);
    ctx.font = `11px ${c.font}`;
    ctx.lineWidth = 1;
    this.grid(ctx, f, c);
    this.axis(ctx, f, c);
    this.line(ctx, f, c);
    if (this.hover >= 0) this.marker(ctx, f, c, this.points[this.hover]);
    else if (this.points.length === 1) this.marker(ctx, f, c, this.points[0]);
  }

  grid(ctx, f, c) {
    ctx.textBaseline = 'middle';
    ctx.textAlign = 'left';
    for (const v of f.y.list) {
      const y = Math.round(f.yOf(v)) + 0.5;
      ctx.strokeStyle = c.grid;
      ctx.beginPath();
      ctx.moveTo(f.x0, y);
      ctx.lineTo(f.x1, y);
      ctx.stroke();
      ctx.fillStyle = c.text;
      ctx.fillText(size(v), f.x1 + 6, y);
    }
  }

  axis(ctx, f, c) {
    const { list, unit } = timeTicks(f.t0, f.t1, f.x1 - f.x0);
    ctx.textBaseline = 'top';
    ctx.textAlign = 'center';
    ctx.fillStyle = c.text;
    ctx.strokeStyle = c.grid;
    for (const t of list) {
      const x = Math.round(f.xOf(t)) + 0.5;
      ctx.beginPath();
      ctx.moveTo(x, f.y1);
      ctx.lineTo(x, f.y1 + 3);
      ctx.stroke();
      ctx.fillText(tickLabel(t, unit), x, f.y1 + 5);
    }
  }

  line(ctx, f, c) {
    const pts = this.points.map((p) => [f.xOf(p.time), f.yOf(p.allocated)]);
    ctx.beginPath();
    pts.forEach(([x, y], i) => (i ? ctx.lineTo(x, y) : ctx.moveTo(x, y)));
    ctx.strokeStyle = c.line;
    ctx.lineWidth = 1.5;
    ctx.lineJoin = 'round';
    ctx.stroke();
    ctx.lineTo(pts.at(-1)[0], f.y1);
    ctx.lineTo(pts[0][0], f.y1);
    ctx.closePath();
    ctx.globalAlpha = 0.12;
    ctx.fillStyle = c.line;
    ctx.fill();
    ctx.globalAlpha = 1;
    ctx.lineWidth = 1;
  }

  marker(ctx, f, c, p) {
    const [x, y] = [f.xOf(p.time), f.yOf(p.allocated)];
    ctx.strokeStyle = c.grid;
    ctx.beginPath();
    ctx.moveTo(Math.round(x) + 0.5, f.y0);
    ctx.lineTo(Math.round(x) + 0.5, f.y1);
    ctx.stroke();
    ctx.beginPath();
    ctx.arc(x, y, 3.5, 0, Math.PI * 2);
    ctx.fillStyle = c.bg;
    ctx.fill();
    ctx.strokeStyle = c.line;
    ctx.lineWidth = 1.5;
    ctx.stroke();
    ctx.lineWidth = 1;
  }
}
