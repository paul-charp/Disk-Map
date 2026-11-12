// Side panel: selection, largest items, age and class totals.
import { api } from './api.js';
import { $, copyText, el } from './dom.js';
import { setIcon } from './icons.js';
import { TAG_COLORS } from './menu.js';
import { AGE_NAMES, css } from './scheme.js';

const REFRESH_MS = 500;

function section(title, extra) {
  const s = el('div', 'sec');
  const h = el('h3', '', title);
  if (extra) h.append(extra);
  s.append(h);
  return s;
}

function kv(pairs) {
  const g = el('div', 'kv');
  for (const [k, v] of pairs) if (v) g.append(el('span', '', k), el('span', '', v));
  return g;
}

function swatch(color) {
  const s = el('span', 'sq');
  s.style.background = color;
  return s;
}

function iconButton(icon, title, run) {
  const b = el('button', 'ib');
  b.title = title;
  setIcon(b, icon);
  b.onclick = run;
  return b;
}

/** Wires the side panel; returns `{update(status)}`. */
export function initSide(app) {
  const root = $('side');
  let showFolders = false;
  let key = '';
  let at = 0;
  let timer = 0;
  let token = 0;

  const nodeColor = (d) => {
    if (d.kind !== 'file') return css(app.scheme.folder(2));
    const mode = app.status.file_colors;
    if (mode === 'age' && d.age != null) return css(app.scheme.file(2, app.scheme.age[d.age]));
    const c = mode === 'class' && app.classes.find((k) => k.name === d.class);
    return c ? c.color : css(app.scheme.file(2));
  };

  const selection = (d) => {
    const s = section('SELECTION');
    if (!d) return s;
    const name = el('div', 'selname');
    name.append(swatch(nodeColor(d)), el('span', '', d.name));
    const parent = el('div', 'selpath', d.path.slice(0, Math.max(0, d.path.length - d.name.length)).replace(/\\$/, '') || d.path);
    const acts = el('div', 'acts');
    for (let i = 1; i <= 4; i++) {
      const dot = el('span', `dot${d.tag === i ? ' on' : ''}`);
      dot.style.background = TAG_COLORS[i - 1];
      dot.onclick = () => app.tag(i, d.id);
      acts.append(dot);
    }
    acts.append(el('span', 'grow'), iconButton('folder', 'Reveal', () => app.reveal(d.id)), iconButton('copy', 'Copy path', () => copyText(d.path)));
    const last = d.kind === 'file' ? ['Class', d.class] : ['Items', d.items?.toLocaleString()];
    s.append(name, parent, kv([['Size', d.size], ['On disk', d.disk], ['Share', d.share], ['Modified', d.modified], last]), acts);
    return s;
  };

  const largest = (st) => {
    const seg = el('span', 'seg');
    for (const [label, on] of [['Files', !showFolders], ['Folders', showFolders]]) {
      const b = el('button', on ? 'on' : '', label);
      b.onclick = () => {
        showFolders = label === 'Folders';
        render();
      };
      seg.append(b);
    }
    const s = section('LARGEST', seg);
    for (const t of showFolders ? st.folders : st.files) {
      const row = el('div', 'li');
      row.title = `${t.parent}\\${t.name}`;
      row.append(el('span', 'n', t.name), el('span', 'v', t.size));
      row.onclick = () => app.select(t.id);
      s.append(row);
    }
    return s;
  };

  const shares = (title, rows) => {
    const s = section(title);
    for (const c of rows.filter((c) => c.share > 0)) {
      const row = el('div', 'li');
      row.append(swatch(c.color), el('span', 'n', c.name), el('span', 'v', c.size));
      const bar = el('div', 'bar');
      const fill = el('i');
      fill.style.cssText = `width:${(c.share * 100).toFixed(1)}%;background:${c.color}`;
      bar.append(fill);
      s.append(row, bar);
    }
    return s;
  };

  const ages = (st) => shares('AGE', st.ages.map((a, i) => ({ ...a, name: AGE_NAMES[i], color: css(app.scheme.file(2, app.scheme.age[i])) })));

  let data = null;
  const render = () => {
    if (!data) return;
    const parts = [selection(data.sel), largest(data.stats)];
    if (app.status.file_colors === 'age') parts.push(ages(data.stats));
    root.replaceChildren(...parts, shares('CLASSES', data.stats.classes));
  };

  const load = async () => {
    const mine = ++token;
    const st = app.status;
    const [stats, sel] = await Promise.all([api.stats(st.tab), st.selection == null ? null : app.details(st.selection)]);
    if (mine !== token) return;
    data = { stats, sel };
    at = performance.now();
    render();
  };

  return {
    update: (st) => {
      root.hidden = !app.cfg.show_side;
      if (root.hidden || !st.active) return;
      const next = `${st.tab}:${st.root}:${st.selection}:${st.ver}:${app.classVer}:${st.file_colors}:${app.cfg.theme}`;
      if (next === key) return;
      key = next;
      clearTimeout(timer);
      const wait = st.running ? Math.max(0, REFRESH_MS - (performance.now() - at)) : 0;
      timer = setTimeout(load, wait);
    },
  };
}
