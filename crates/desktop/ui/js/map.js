// The map canvas: layout requests, base bitmap, hover overlay, zoom animation and pointer input.
import { api } from './api.js';
import { F, PaintList } from './paintlist.js';
import { Painter } from './render.js';
import { css } from './scheme.js';

const ZOOM_MS = 200;
const MIN_INTERVAL = 100;
const lerp = (a, b, t) => a + (b - a) * t;

export class MapView {
  /** `handlers`: onHover(node, e), onMove(e), onLeave(), onSelect(node), onZoom(node), onMenu(node, e), onBack(), onForward(). */
  constructor(canvas, { classes, scheme, textScale, animate, handlers, onPerf }) {
    this.canvas = canvas;
    this.ctx = canvas.getContext('2d', { alpha: false });
    this.handlers = handlers;
    this.onPerf = onPerf;
    this.scheme = scheme;
    this.painter = new Painter(this.scheme, classes);
    this.painter.text = textScale;
    this.animate = animate;
    this.tab = null;
    this.saved = new Map();
    this.cur = null;
    this.hover = -1;
    this.pointer = null;
    this.forced = null;
    this.forcedTip = false;
    this.anim = null;
    this.animFrom = null;
    this.flashing = false;
    this.busy = false;
    this.again = null;
    this.timer = 0;
    this.cost = 0;
    this.lastEnd = 0;
    this.pool = [];
    this.dpr = window.devicePixelRatio || 1;
    this.setSize(canvas.width, canvas.height);
    this.#watchSize();
    this.#bindPointer();
  }

  /** Switches the colour scheme and repaints. */
  setScheme(scheme) {
    this.scheme = scheme;
    this.painter.scheme = scheme;
    this.refresh();
  }

  /** Changes the label size scale; the caller refreshes. */
  setText(scale) {
    this.painter.setDpr(this.dpr, scale);
  }

  /** Replaces the class colours; the caller refreshes. */
  setClasses(classes) {
    this.painter.setClasses(classes);
  }

  /** Shows another tab: its last paint list right away, a fresh layout next. */
  switchTo(tab) {
    if (tab === this.tab) return;
    if (this.tab !== null && this.cur) this.saved.set(this.tab, this.cur);
    this.#endAnim();
    this.tab = tab;
    this.cur = this.saved.get(tab) ?? null;
    this.saved.delete(tab);
    this.animFrom = null;
    this.hover = -1;
    this.forced = null;
    if (this.cur && this.cur.base.width === this.canvas.width && this.cur.base.height === this.canvas.height) return this.#paint();
    this.cur = null;
    this.ctx.fillStyle = css(this.scheme.bg);
    this.ctx.fillRect(0, 0, this.canvas.width, this.canvas.height);
  }

  /** Drops what is kept for a closed tab. */
  forget(tab) {
    this.saved.delete(tab);
    if (this.tab === tab) {
      this.tab = null;
      this.cur = null;
    }
  }

  /** Node under the pointer, or null. */
  hoverNode() {
    return this.hover >= 0 ? this.cur.list.node(this.hover) : null;
  }

  setSize(w, h) {
    const dpr = window.devicePixelRatio || 1;
    this.dpr = dpr;
    this.painter.setDpr(dpr);
    this.canvas.width = w;
    this.canvas.height = h;
    this.mw = Math.max(1, w);
    this.mh = Math.max(1, h);
    this.pool = [];
    if (this.cur) this.#paint();
  }

  #watchSize() {
    const ro = new ResizeObserver(([e]) => {
      const box = e.devicePixelContentBoxSize?.[0];
      const dpr = window.devicePixelRatio || 1;
      this.#resize(
        box ? box.inlineSize : Math.round(e.contentRect.width * dpr),
        box ? box.blockSize : Math.round(e.contentRect.height * dpr),
        dpr,
      );
    });
    ro.observe(this.canvas, { box: 'device-pixel-content-box' });
    window.addEventListener('resize', () => this.syncSize());
  }

  /** Re-reads the canvas size and the device pixel ratio, e.g. after a zoom change that kept the pixel size. */
  syncSize() {
    const r = this.canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    this.#resize(Math.round(r.width * dpr), Math.round(r.height * dpr), dpr);
  }

  #resize(w, h, dpr) {
    if (w === this.canvas.width && h === this.canvas.height && dpr === this.dpr) return;
    this.setSize(w, h);
    this.request();
  }

  // ----- layout and painting -----

  /** Schedules a refresh, throttled to the cost of the previous one. */
  request(animate = false) {
    clearTimeout(this.timer);
    const wait = Math.max(MIN_INTERVAL, this.cost * 4) - (performance.now() - this.lastEnd);
    this.timer = setTimeout(() => this.refresh({ animate }), Math.max(0, wait));
  }

  /** Fetches a fresh layout and paints it, animating a root change when asked. */
  async refresh({ animate = false } = {}) {
    animate = animate && this.animate;
    if (animate && !this.animFrom) this.animFrom = this.cur;
    if (this.busy) {
      this.again = { animate: animate || !!this.again?.animate };
      return;
    }
    if (this.tab === null) return;
    this.busy = true;
    const tab = this.tab;
    try {
      const t0 = performance.now();
      const buf = await api.layout(tab, this.mw, this.mh, this.dpr);
      const t1 = performance.now();
      if (tab !== this.tab) return;
      const list = new PaintList(buf);
      const t2 = performance.now();
      const base = this.#renderBase(list);
      const t3 = performance.now();
      this.#swap(list, base, animate);
      this.onPerf?.({ items: list.n, backend: list.backendMicros / 1000, trip: t1 - t0, decode: t2 - t1, paint: t3 - t2 });
      this.cost = performance.now() - t0;
    } catch (e) {
      console.debug('layout', e);
    } finally {
      this.busy = false;
      this.lastEnd = performance.now();
      if (this.again) {
        const next = this.again;
        this.again = null;
        this.refresh(next);
      }
    }
  }

  #canvasFor() {
    const c = this.pool.pop() ?? document.createElement('canvas');
    if (c.width !== this.mw) c.width = this.mw;
    if (c.height !== this.mh) c.height = this.mh;
    return c;
  }

  #renderBase(list) {
    const c = this.#canvasFor();
    const g = c.getContext('2d', { alpha: false });
    g.fillStyle = css(this.scheme.bg);
    g.fillRect(0, 0, c.width, c.height);
    this.painter.base(g, list);
    return c;
  }

  #swap(list, base, animate) {
    const prev = this.cur;
    if (this.anim && prev.list.root === list.root) return this.#refine(list, base);
    this.#endAnim();
    this.cur = { list, base };
    const from = this.animFrom;
    const changed = from && from.list.root !== list.root;
    const anim = changed ? this.#plan(from, list) : null;
    if (changed || animate) this.animFrom = null;
    const keep = [this.animFrom, anim && from];
    for (const old of new Set([prev, from])) if (old && !keep.includes(old)) this.pool.push(old.base);
    this.#settleHover();
    if (anim) {
      this.anim = { ...anim, from, t0: performance.now() };
      requestAnimationFrame((t) => this.#tick(t));
    } else {
      this.#paint();
    }
  }

  /** Swaps in a fresher layout of the same root without interrupting the zoom animation. */
  #refine(list, base) {
    this.pool.push(this.cur.base);
    if (this.animFrom === this.cur) this.animFrom = null;
    this.cur = { list, base };
    this.#settleHover();
  }

  /** Zoom direction and the rect that grows to or shrinks from the full view. */
  #plan(prev, list) {
    const into = prev.list.byNode.get(list.root);
    if (into > 0) return { kind: 'in', rect: this.#rectOf(prev.list, into) };
    const out = list.byNode.get(prev.list.root);
    return out > 0 ? { kind: 'out', rect: this.#rectOf(list, out) } : null;
  }

  #rectOf(list, i) {
    return { x: list.x(i), y: list.y(i), w: list.w(i), h: list.h(i) };
  }

  #tick(now) {
    const a = this.anim;
    if (!a) return;
    const t = Math.min(1, (now - a.t0) / ZOOM_MS);
    const e = 1 - (1 - t) ** 3;
    const full = { x: 0, y: 0, w: this.mw, h: this.mh };
    const [still, moving, r0, r1] = a.kind === 'in'
      ? [a.from.base, this.cur.base, a.rect, full]
      : [this.cur.base, a.from.base, full, a.rect];
    const r = { x: lerp(r0.x, r1.x, e), y: lerp(r0.y, r1.y, e), w: lerp(r0.w, r1.w, e), h: lerp(r0.h, r1.h, e) };
    this.#frame(still);
    this.ctx.drawImage(moving, r.x, r.y, r.w, r.h);
    if (t < 1) requestAnimationFrame((n) => this.#tick(n));
    else this.#endAnim();
  }

  #endAnim() {
    if (!this.anim) return;
    this.pool.push(this.anim.from.base);
    this.anim = null;
    if (this.cur) this.#paint();
  }

  #frame(img) {
    const g = this.ctx;
    g.fillStyle = css(this.scheme.bg);
    g.fillRect(0, 0, this.canvas.width, this.canvas.height);
    g.drawImage(img, 0, 0);
  }

  #paint() {
    if (this.anim || !this.cur) return;
    this.#frame(this.cur.base);
    if (this.hover >= 0) this.painter.hover(this.ctx, this.cur.list, this.hover);
    this.#flash();
  }

  /** Draws the change highlight over the cached base and keeps animating until it has faded. */
  #flash() {
    const list = this.cur.list;
    const now = performance.now();
    if (!list.marked.length || now >= list.fadeEnd) return;
    this.painter.highlight(this.ctx, list, now);
    if (this.flashing) return;
    this.flashing = true;
    requestAnimationFrame(() => {
      this.flashing = false;
      this.#paint();
    });
  }

  // ----- pointer -----

  #pos(e) {
    const r = this.canvas.getBoundingClientRect();
    return [
      ((e.clientX - r.left) * this.canvas.width) / r.width,
      ((e.clientY - r.top) * this.canvas.height) / r.height,
    ];
  }

  #hit(e) {
    return this.cur ? this.cur.list.hit(...this.#pos(e)) : -1;
  }

  #setHover(i, e) {
    if (i === this.hover) return;
    this.hover = i;
    this.#paint();
    this.handlers.onHover(this.hoverNode(), e);
  }

  /** Hovers a node without a pointer, for tests; `tip` also shows its tooltip. Cleared by the next pointer move. */
  hoverTo(node, tip = false) {
    this.forced = node;
    this.forcedTip = tip;
    this.#settleHover();
  }

  #settleHover() {
    const list = this.cur.list;
    const i = this.pointer ? list.hit(...this.#pos(this.pointer)) : (list.byNode.get(this.forced) ?? -1);
    if (i !== this.hover) this.#setHover(i, this.pointer ?? (this.forcedTip ? this.#centerOf(i) : null));
  }

  /** Pointer-like position at the centre of an item, for forced hovers. */
  #centerOf(i) {
    if (i < 0) return null;
    const r = this.canvas.getBoundingClientRect();
    const l = this.cur.list;
    return {
      clientX: r.left + (l.x(i) + l.w(i) / 2) * (r.width / this.canvas.width),
      clientY: r.top + (l.y(i) + l.h(i) / 2) * (r.height / this.canvas.height),
    };
  }

  #bindPointer() {
    const c = this.canvas;
    let first = null;
    c.addEventListener('mousemove', (e) => {
      this.pointer = e;
      this.forced = null;
      this.#setHover(this.#hit(e), e);
      this.handlers.onMove(e);
    });
    c.addEventListener('mouseleave', () => {
      this.pointer = null;
      this.#setHover(-1, null);
      this.handlers.onLeave();
    });
    c.addEventListener('click', (e) => {
      if (e.detail > 1) return;
      const i = this.#hit(e);
      const node = i >= 0 ? this.cur.list.node(i) : -1;
      first = i > 0 && this.cur.list.flags(i) & F.DIR ? node : null;
      this.handlers.onSelect(node >= 0 ? node : null);
    });
    c.addEventListener('dblclick', () => first !== null && this.handlers.onZoom(first));
    c.addEventListener('contextmenu', (e) => {
      e.preventDefault();
      const i = this.#hit(e);
      this.handlers.onMenu(i >= 0 ? this.cur.list.node(i) : null, e);
    });
    const side = (e) => {
      if (e.button < 3) return;
      e.preventDefault();
      if (e.type === 'mouseup') (e.button === 3 ? this.handlers.onBack : this.handlers.onForward)();
    };
    for (const type of ['mousedown', 'mouseup', 'auxclick']) window.addEventListener(type, side);
  }
}
