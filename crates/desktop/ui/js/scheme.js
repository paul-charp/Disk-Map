// Map colour schemes, one per chrome theme. A scheme turns an element kind and nesting depth into colours.
const clamp = (v) => Math.max(0, Math.min(255, Math.round(v)));
const mix = (a, b, t) => a.map((v, i) => clamp(v + (b[i] - v) * t));
const WHITE = [255, 255, 255];
const BLACK = [0, 0, 0];
const INK = [12, 13, 16];
const MIN_CONTRAST = 5.5;

const cssCache = new Map();
/** `rgb(...)` string of a colour, cached. */
export function css(c) {
  const key = (c[0] << 16) | (c[1] << 8) | c[2];
  let s = cssCache.get(key);
  if (!s) cssCache.set(key, (s = `rgb(${c[0]},${c[1]},${c[2]})`));
  return s;
}

const pack = (c) => (c[0] << 16) | (c[1] << 8) | c[2];

/** Names of the age buckets, most recent first. */
export const AGE_NAMES = ['< 1 week', '< 1 month', '< 6 months', '< 1 year', '< 3 years', 'Older'];

/** Caches a function of one number or colour by `key(arg)`. */
function memo(fn, key) {
  const cache = new Map();
  return (arg, ...rest) => {
    const k = key(arg, ...rest);
    let v = cache.get(k);
    if (v === undefined) cache.set(k, (v = fn(arg, ...rest)));
    return v;
  };
}

/** Parses `#rrggbb`. */
export function parseHex(h) {
  const v = parseInt(h.slice(1), 16);
  return [v >> 16, (v >> 8) & 255, v & 255];
}

function luminance([r, g, b]) {
  const f = (v) => (v / 255 <= 0.03928 ? v / 255 / 12.92 : ((v / 255 + 0.055) / 1.055) ** 2.4);
  return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
}

const contrast = (a, b) => {
  const [la, lb] = [luminance(a), luminance(b)];
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
};

const bestContrast = (c) => Math.max(contrast(c, INK), contrast(c, WHITE));

/** Moves a fill towards black or white until a label colour reads on it. */
function legible(c) {
  for (const target of [BLACK, WHITE]) {
    let out = c;
    for (let i = 0; i < 20 && bestContrast(out) < MIN_CONTRAST; i++) out = mix(out, target, 0.05);
    if (bestContrast(out) >= MIN_CONTRAST) return out;
  }
  return c;
}

/** Builds a scheme from a theme's map palette. */
function makeScheme(p) {
  /** Share of the way from the background to the hue at a nesting depth. */
  const level = (depth) => Math.min(0.32 + 0.17 * (Math.max(depth, 1) - 1), 0.97);
  const toward = (hue, depth) => legible(mix(p.bg, hue, level(depth)));
  return {
    name: p.name,
    bg: p.bg,
    accent: p.accent,
    /** File hues per age bucket, most recent first. */
    age: p.age,
    free: legible(p.free),
    unknown: legible(p.unknown),
    /** Fill of an open folder panel or a closed folder tile at a depth below the view root. */
    folder: memo((depth) => toward(p.folder, depth), (d) => d),
    /** Fill of a file tile; `hue` overrides the file hue, e.g. with a class colour. */
    file: memo((depth, hue = p.file) => toward(hue, depth), (d, hue = p.file) => d * 16777216 + pack(hue)),
    /** Near-black or white, whichever reads better. */
    text: memo((c) => (contrast(c, INK) >= contrast(c, WHITE) ? css(INK) : '#fff'), pack),
    /** Lighter tint of a hovered tile's own fill. */
    hover: memo((c) => legible(mix(c, WHITE, 0.3)), pack),
  };
}

/** `age` is the file hue per age bucket, most recent first (see `diskmap_core::age`). */
const palette = (name, dark, bg, folder, file, free, unknown, accent, age) => ({
  name, dark, bg: parseHex(bg), folder: parseHex(folder), file: parseHex(file), free: parseHex(free), unknown: parseHex(unknown), accent: parseHex(accent),
  age: age.map(parseHex),
});

export const SCHEMES = Object.fromEntries(
  [
    palette('graphite', true, '#131416', '#5d6573', '#3f78b8', '#4a6656', '#2c2d32', '#4c9aff',
      ['#f08c3c', '#e3b341', '#8fbf5a', '#3fa39a', '#3f78b8', '#4b5160']),
    palette('paper', false, '#c5c9d0', '#e4e7ec', '#9ec1e6', '#bfd3c6', '#b0b3ba', '#0b63ce',
      ['#f3a467', '#f0d27a', '#b9db92', '#94d1c6', '#9ec1e6', '#d6d9de']),
    palette('dusk', true, '#12161d', '#566478', '#3a8fa8', '#44665a', '#262c38', '#62a8f0',
      ['#f0904b', '#e3c45a', '#8fc46a', '#4fb0a8', '#3a8fa8', '#4a5468']),
    palette('ember', true, '#151311', '#5f5b57', '#c0672f', '#4f6655', '#2a2724', '#d9825b',
      ['#f26b2e', '#e0a040', '#c2b45a', '#7f9a6a', '#5c7a8a', '#4a4643']),
    palette('violet', true, '#121215', '#605c78', '#7a5fd0', '#4a6458', '#27272e', '#a78bfa',
      ['#f0806a', '#e6b45a', '#9fca6a', '#56b0b8', '#7a5fd0', '#4b4762']),
  ].map((p) => [p.name, makeScheme(p)]),
);
