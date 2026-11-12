// Output panel: the application log with a level filter, and its counts in the status bar.
import { api, onLog } from './api.js';
import { $, copyText, el } from './dom.js';

const MAX = 1000;
const MIN_HEIGHT = 80;
const MAP_ROOM = 160;
const RANK = { error: 0, warn: 1, info: 2 };
const LABEL = { error: 'ERROR', warn: 'WARN', info: 'INFO' };

const pad = (n) => String(n).padStart(2, '0');
const clock = (ms) => {
  const d = new Date(ms);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
};
const line = (e) => `${clock(e.time)} ${LABEL[e.level]} ${e.source}: ${e.message}`;

function row(e) {
  const r = el('div', `lrow ${e.level}`);
  r.append(el('span', 't', clock(e.time)), el('span', 'l', LABEL[e.level]), el('span', 's', e.source), el('span', 'm', e.message));
  r.title = new Date(e.time).toLocaleString();
  return r;
}

function setCount(id, n) {
  const c = $(id);
  c.lastElementChild.textContent = String(n);
  c.classList.toggle('on', n > 0);
}

/** Lets the top edge of the panel resize it. */
function bindSash(panel) {
  const sash = $('out-sash');
  sash.addEventListener('pointerdown', (e) => {
    e.preventDefault();
    sash.setPointerCapture(e.pointerId);
    sash.classList.add('drag');
    const [y0, h0] = [e.clientY, panel.offsetHeight];
    const move = (m) => {
      const h = Math.min(innerHeight - MAP_ROOM, h0 + y0 - m.clientY);
      panel.style.height = `${Math.max(MIN_HEIGHT, h)}px`;
    };
    const up = () => {
      sash.classList.remove('drag');
      sash.removeEventListener('pointermove', move);
      sash.removeEventListener('pointerup', up);
    };
    sash.addEventListener('pointermove', move);
    sash.addEventListener('pointerup', up);
  });
}

/** Loads the log and follows it; returns `{toggle(), show(), isOpen()}`. */
export async function initOutput() {
  const panel = $('output');
  const list = $('out-list');
  const seg = $('out-level');
  let entries = [];
  let level = 'info';
  let queued = false;

  const shown = () => entries.filter((e) => RANK[e.level] <= RANK[level]);

  const render = () => {
    queued = false;
    setCount('st-log-e', entries.filter((e) => e.level === 'error').length);
    setCount('st-log-w', entries.filter((e) => e.level === 'warn').length);
    if (panel.hidden) return;
    const atEnd = list.scrollTop + list.clientHeight >= list.scrollHeight - 4;
    const rows = shown().map(row);
    list.replaceChildren(...(rows.length ? rows : [el('div', 'lempty', 'No entries')]));
    if (atEnd) list.scrollTop = list.scrollHeight;
  };
  const schedule = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(render);
  };
  const add = (e) => {
    entries.push(e);
    if (entries.length > MAX) entries.splice(0, entries.length - MAX);
    schedule();
  };

  const setLevel = (value) => {
    level = value;
    for (const b of seg.children) b.classList.toggle('on', b.dataset.level === value);
    render();
  };
  const setOpen = (open) => {
    panel.hidden = !open;
    $('st-log').classList.toggle('on', open);
    render();
    if (open) list.scrollTop = list.scrollHeight;
  };

  for (const b of seg.children) b.onclick = () => setLevel(b.dataset.level);
  $('st-log').onclick = () => setOpen(panel.hidden);
  $('out-close').onclick = () => setOpen(false);
  $('out-copy').onclick = () => copyText(shown().map(line).join('\n'));
  $('out-clear').onclick = () => {
    entries = [];
    api.logClear();
    render();
  };
  bindSash(panel);
  setLevel(level);

  const early = [];
  let ready = false;
  await onLog((e) => (ready ? add(e) : early.push(e)));
  entries = await api.logEntries().catch(() => []);
  const last = entries.at(-1)?.id ?? -1;
  entries.push(...early.filter((e) => e.id > last));
  ready = true;
  render();

  return {
    toggle: () => setOpen(panel.hidden),
    show: () => setOpen(true),
    isOpen: () => !panel.hidden,
  };
}
