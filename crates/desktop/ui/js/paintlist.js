// Decoder of the binary paint list produced by `paint.rs`.
export const F = { FILE: 1, DIR: 2, DRILLED: 4, FREE: 8, UNKNOWN: 16, SELECTED: 32, BOLD: 64, LABEL: 128 };
export const TAG_SHIFT = 8;
const HEADER_INTS = 4;
const STRIDE = 8;
const NO_CLASS = 255;
const HL_WARN = 1 << 15;
const HL_AGE = HL_WARN - 1;
/** Milliseconds a changed item takes to fade out. */
export const HL_FADE_MS = 1500;

/** One decoded layout: items in paint order with parent links and labels. */
export class PaintList {
  constructor(buf) {
    const head = new Int32Array(buf, 0, HEADER_INTS);
    this.n = head[0];
    this.root = head[2];
    this.backendMicros = head[3];
    this.ints = new Int32Array(buf, HEADER_INTS * 4, this.n * STRIDE);
    const blobAt = (HEADER_INTS + this.n * STRIDE) * 4;
    const blob = new TextDecoder().decode(new Uint8Array(buf, blobAt, head[1]));
    this.labels = head[1] ? blob.split('\n') : [];
    this.labelOf = new Int32Array(this.n).fill(-1);
    this.stamp = performance.now();
    this.marked = [];
    this.fadeEnd = this.stamp;
    this.#index();
  }

  #index() {
    let k = 0;
    for (let i = 0; i < this.n; i++) {
      if (this.flags(i) & F.LABEL) this.labelOf[i] = k++;
      const age = this.hlAge(i);
      if (i > 0 && this.hlWeight(i) && age < HL_FADE_MS) {
        this.marked.push(i);
        this.fadeEnd = Math.max(this.fadeEnd, this.stamp + HL_FADE_MS - age);
      }
    }
  }

  /** Item index by node id, built on first use. */
  get byNode() {
    if (!this.map) {
      this.map = new Map();
      for (let i = 0; i < this.n; i++) this.map.set(this.node(i), i);
    }
    return this.map;
  }

  x(i) { return this.ints[i * STRIDE]; }
  y(i) { return this.ints[i * STRIDE + 1]; }
  w(i) { return this.ints[i * STRIDE + 2]; }
  h(i) { return this.ints[i * STRIDE + 3]; }
  node(i) { return this.ints[i * STRIDE + 4]; }
  parent(i) { return this.ints[i * STRIDE + 6]; }
  depth(i) { return this.ints[i * STRIDE + 5] & 255; }
  cls(i) { const c = (this.ints[i * STRIDE + 5] >> 8) & 255; return c === NO_CLASS ? -1 : c; }
  flags(i) { return this.ints[i * STRIDE + 5] >>> 16; }
  tag(i) { return (this.flags(i) >> TAG_SHIFT) & 7; }

  /** Strength 0..255 of the change highlight of an item. */
  hlWeight(i) { return this.ints[i * STRIDE + 7] >>> 16; }
  /** True when the change was a shrink or a removal. */
  hlWarn(i) { return (this.ints[i * STRIDE + 7] & HL_WARN) !== 0; }
  /** Age in milliseconds of the change when the layout was made. */
  hlAge(i) { return this.ints[i * STRIDE + 7] & HL_AGE; }

  /** `[name, size]` of a tile label, or the single header text of a drilled folder. */
  label(i) {
    const k = this.labelOf[i];
    return k < 0 ? null : this.labels[k].split('\u0001');
  }

  /** Deepest item containing the point, or -1. */
  hit(px, py) {
    const a = this.ints;
    for (let i = this.n - 1; i >= 0; i--) {
      const o = i * STRIDE;
      if (px >= a[o] && py >= a[o + 1] && px < a[o] + a[o + 2] && py < a[o + 1] + a[o + 3]) return i;
    }
    return -1;
  }
}
