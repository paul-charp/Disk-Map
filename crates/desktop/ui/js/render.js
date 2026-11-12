// Draws paint-list items on a canvas, in device pixels.
import { F, HL_FADE_MS } from './paintlist.js';
import { css, parseHex } from './scheme.js';

const UI = '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif';
const MONO = '"Cascadia Mono", Consolas, monospace';
const TAG_COLORS = [null, '#e03131', '#f2c230', '#2f9e44', '#1c7ed6'];
const SUB_ALPHA = 0.78;
const ELLIPSIS = '…';
const HL_ALPHA = 0.55;
const HL_STEPS = 12;
const WARN = '#e8590c';

/** Pixel metrics of the map for a device pixel ratio and a text scale, in whole device pixels. Mirrors `metrics` in `paint.rs`. */
export function metricsFor(dpr, text = 1) {
  const px = (v) => Math.round(v * dpr);
  const pt = (v) => Math.round(v * dpr * text);
  return {
    name: pt(12),
    size: pt(11),
    line: pt(16),
    padX: px(6),
    padY: px(5),
    head: pt(18),
    tileRadius: px(3),
    panelRadius: px(4),
    flatBelow: px(10),
    outline: Math.max(2, px(1.4)),
    dot: px(3),
    twoLines: pt(40),
    nameMin: px(24),
    hatch: px(8),
  };
}

function addRect(ctx, x, y, w, h, r) {
  if (r > 0) ctx.roundRect(x, y, w, h, r);
  else ctx.rect(x, y, w, h);
}

function roundRect(ctx, x, y, w, h, r) {
  ctx.beginPath();
  addRect(ctx, x, y, w, h, r);
}

/** Text widths from per-character widths, so each distinct character is measured once per font. */
class TextMeter {
  constructor() {
    this.fonts = new Map();
  }

  #widths(font) {
    let w = this.fonts.get(font);
    if (!w) this.fonts.set(font, (w = new Map()));
    return w;
  }

  /** Width of one character in the context's current font. */
  #char(ctx, widths, ch) {
    let v = widths.get(ch);
    if (v === undefined) widths.set(ch, (v = ctx.measureText(ch).width));
    return v;
  }

  /** Fits text into `max` pixels, ending with an ellipsis when cut; the context font must be `font`. */
  fit(ctx, font, text, max) {
    const widths = this.#widths(font);
    let total = 0;
    for (let i = 0; i < text.length; i++) total += this.#char(ctx, widths, text[i]);
    if (total <= max) return text;
    const room = max - this.#char(ctx, widths, ELLIPSIS);
    let used = 0;
    let n = 0;
    while (n < text.length && used + this.#char(ctx, widths, text[n]) <= room) used += this.#char(ctx, widths, text[n++]);
    return n ? `${text.slice(0, n)}${ELLIPSIS}` : '';
  }

  /** Width of text in the context's current font. */
  width(ctx, font, text) {
    const widths = this.#widths(font);
    let total = 0;
    for (let i = 0; i < text.length; i++) total += this.#char(ctx, widths, text[i]);
    return total;
  }
}

/** Paints items of one list with one scheme. */
export class Painter {
  constructor(scheme, classes) {
    this.scheme = scheme;
    this.setClasses(classes);
    this.meter = new TextMeter();
    this.dpr = 1;
    this.text = 1;
    this.m = metricsFor(1);
    this.font = '';
    this.fill = '';
  }

  setClasses(classes) {
    this.classRgb = classes.map((c) => parseHex(c.color));
  }

  setDpr(dpr, text = this.text) {
    this.dpr = dpr;
    this.text = text;
    this.m = metricsFor(dpr, text);
  }

  /** Base colour of an item. */
  color(list, i) {
    const f = list.flags(i);
    if (f & F.FREE) return this.scheme.free;
    if (f & F.UNKNOWN) return this.scheme.unknown;
    const d = list.depth(i);
    if (f & F.FILE) {
      const c = list.cls(i);
      if (c < 0) return this.scheme.file(d);
      return this.scheme.file(d, f & F.AGE ? this.scheme.age[c] : this.classRgb[c]);
    }
    return this.scheme.folder(d);
  }

  /** Paints every item below the view root: fills batched per depth and colour, then labels and marks. */
  base(ctx, list) {
    const marked = [];
    for (const [fill, items] of this.#batches(list, marked)) this.#fillBatch(ctx, list, fill, items);
    this.font = '';
    this.fill = '';
    for (const i of marked) this.#decor(ctx, list, i, this.color(list, i));
  }

  /** Item indices grouped by depth, then fill colour, in paint order; fills items that need decoration into `marked`. */
  #batches(list, marked) {
    const groups = new Map();
    const bare = F.LABEL | F.UNKNOWN | F.SELECTED | (7 << 8);
    for (let i = 1; i < list.n; i++) {
      const c = this.color(list, i);
      const key = list.depth(i) * 16777216 + ((c[0] << 16) | (c[1] << 8) | c[2]);
      let g = groups.get(key);
      if (!g) groups.set(key, (g = { fill: css(c), items: [] }));
      g.items.push(i);
      if (list.flags(i) & bare) marked.push(i);
    }
    return [...groups.entries()].sort((a, b) => a[0] - b[0]).map(([, g]) => [g.fill, g.items]);
  }

  #fillBatch(ctx, list, fill, items) {
    const m = this.m;
    ctx.fillStyle = fill;
    ctx.beginPath();
    for (const i of items) {
      const [w, h] = [list.w(i), list.h(i)];
      const panel = list.flags(i) & F.DRILLED;
      const r = panel ? m.panelRadius : Math.min(w, h) < m.flatBelow ? 0 : m.tileRadius;
      addRect(ctx, list.x(i), list.y(i), w, h, r);
    }
    ctx.fill();
  }

  /** Lighter tint of the hovered item, with its contents drawn again on top. */
  hover(ctx, list, i) {
    if (i < 1) return;
    this.font = '';
    this.fill = '';
    this.#item(ctx, list, i, this.scheme.hover(this.color(list, i)));
    if (!(list.flags(i) & F.DRILLED)) return;
    const [x, y, w, h] = [list.x(i), list.y(i), list.w(i), list.h(i)];
    for (let j = i + 1; j < list.n; j++) {
      const inside = list.x(j) >= x && list.y(j) >= y && list.x(j) + list.w(j) <= x + w && list.y(j) + list.h(j) <= y + h;
      if (inside) this.#item(ctx, list, j, this.color(list, j));
    }
  }

  /** Tints the recently changed items: whole tiles, only the header of open folders. Fades out by age. */
  highlight(ctx, list, now) {
    const m = this.m;
    const elapsed = now - list.stamp;
    const groups = new Map();
    for (const i of list.marked) {
      const k = 1 - (list.hlAge(i) + elapsed) / HL_FADE_MS;
      if (k <= 0) continue;
      const step = Math.round((list.hlWeight(i) / 255) * k * k * HL_STEPS);
      if (!step) continue;
      const key = step * 2 + (list.hlWarn(i) ? 1 : 0);
      if (!groups.has(key)) groups.set(key, []);
      groups.get(key).push(i);
    }
    for (const [key, items] of groups) {
      ctx.globalAlpha = (HL_ALPHA * (key >> 1)) / HL_STEPS;
      ctx.fillStyle = key & 1 ? WARN : css(this.scheme.accent);
      ctx.beginPath();
      for (const i of items) {
        const head = list.flags(i) & F.DRILLED;
        addRect(ctx, list.x(i), list.y(i), list.w(i), head ? Math.min(m.head, list.h(i)) : list.h(i), this.#radius(list, i));
      }
      ctx.fill();
    }
    ctx.globalAlpha = 1;
  }

  #radius(list, i) {
    const m = this.m;
    return list.flags(i) & F.DRILLED ? m.panelRadius : Math.min(list.w(i), list.h(i)) < m.flatBelow ? 0 : m.tileRadius;
  }

  #item(ctx, list, i, fill) {
    ctx.fillStyle = css(fill);
    this.fill = '';
    roundRect(ctx, list.x(i), list.y(i), list.w(i), list.h(i), this.#radius(list, i));
    ctx.fill();
    this.#decor(ctx, list, i, fill);
  }

  /** Hatching, label, tag dot and selection outline of an item whose fill is already painted. */
  #decor(ctx, list, i, fill) {
    const [x, y, w, h] = [list.x(i), list.y(i), list.w(i), list.h(i)];
    const f = list.flags(i);
    const panel = f & F.DRILLED;
    const r = this.#radius(list, i);
    if (f & F.UNKNOWN) this.#hatch(ctx, [x, y, w, h], r, this.scheme.text(fill));
    const label = list.label(i);
    if (label) {
      const color = f & F.SELECTED && panel ? css(this.scheme.accent) : this.scheme.text(fill);
      if (panel) this.#header(ctx, [x, y, w], label, color, f & F.BOLD);
      else this.#tileLabel(ctx, [x, y, w, h], label, color, f & F.BOLD);
    }
    this.#tagDot(ctx, list, i);
    if (f & F.SELECTED) this.#outline(ctx, [x, y, w, h], r);
  }

  #setFont(ctx, font) {
    if (font === this.font) return;
    this.font = font;
    ctx.font = font;
  }

  #setFill(ctx, color) {
    if (color === this.fill) return;
    this.fill = color;
    ctx.fillStyle = color;
  }

  #hatch(ctx, [x, y, w, h], r, color) {
    const step = this.m.hatch;
    ctx.save();
    roundRect(ctx, x, y, w, h, r);
    ctx.clip();
    ctx.strokeStyle = color;
    ctx.globalAlpha = 0.14;
    ctx.lineWidth = Math.max(1, Math.round(step / 8));
    ctx.beginPath();
    for (let k = -h; k < w; k += step) {
      ctx.moveTo(x + k, y + h);
      ctx.lineTo(x + k + h, y);
    }
    ctx.stroke();
    ctx.restore();
  }

  #outline(ctx, [x, y, w, h], r) {
    const t = this.m.outline;
    this.#setFill(ctx, css(this.scheme.accent));
    ctx.beginPath();
    addRect(ctx, x, y, w, h, r);
    const inner = Math.max(0, r - t);
    addRect(ctx, x + t, y + t, w - 2 * t, h - 2 * t, inner);
    ctx.fill('evenodd');
  }

  #header(ctx, [x, y, w], [name, size], color, bold) {
    const m = this.m;
    const top = y + Math.round((m.head - m.name) / 2) - 1;
    const inner = w - 2 * m.padX;
    ctx.textBaseline = 'top';
    ctx.textAlign = 'left';
    const mono = `${m.size}px ${MONO}`;
    const sans = `${bold ? 700 : 600} ${m.name}px ${UI}`;
    this.#setFont(ctx, mono);
    const sizeW = this.meter.width(ctx, mono, size);
    const showSize = inner - sizeW - m.padX >= m.nameMin;
    this.#setFont(ctx, sans);
    this.#setFill(ctx, color);
    ctx.fillText(this.meter.fit(ctx, sans, name, showSize ? inner - sizeW - m.padX : inner), x + m.padX, top);
    if (!showSize) return;
    this.#setFont(ctx, mono);
    ctx.globalAlpha = SUB_ALPHA;
    ctx.textAlign = 'right';
    ctx.fillText(size, x + w - m.padX, top + Math.round((m.name - m.size) / 2));
    ctx.globalAlpha = 1;
    ctx.textAlign = 'left';
  }

  #tileLabel(ctx, [x, y, w, h], [name, size], color, bold) {
    const m = this.m;
    const inner = w - 2 * m.padX;
    ctx.textBaseline = 'top';
    ctx.textAlign = 'left';
    this.#setFill(ctx, color);
    const sans = `${bold ? 700 : 500} ${m.name}px ${UI}`;
    this.#setFont(ctx, sans);
    ctx.fillText(this.meter.fit(ctx, sans, name, inner), x + m.padX, y + m.padY);
    if (!size || h < m.twoLines) return;
    const mono = `${m.size}px ${MONO}`;
    this.#setFont(ctx, mono);
    ctx.globalAlpha = SUB_ALPHA;
    ctx.fillText(this.meter.fit(ctx, mono, size, inner), x + m.padX, y + m.padY + m.line);
    ctx.globalAlpha = 1;
  }

  #tagDot(ctx, list, i) {
    const tag = list.tag(i);
    const m = this.m;
    if (!tag || list.w(i) < 4 * m.dot) return;
    this.#setFill(ctx, TAG_COLORS[tag]);
    ctx.beginPath();
    ctx.arc(list.x(i) + list.w(i) - 2 * m.dot, list.y(i) + 2 * m.dot, m.dot, 0, Math.PI * 2);
    ctx.fill();
  }
}
