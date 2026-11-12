// Status bar: path of the hovered or selected element, scan counters.
import { api } from './api.js';
import { $ } from './dom.js';

const MSG_MS = 6000;

const probe = document.createElement('canvas').getContext('2d');

/** Shortens text to `max` pixels by replacing its middle with an ellipsis. */
function elide(text, max, font) {
  probe.font = font;
  if (probe.measureText(text).width <= max) return text;
  let keep = text.length - 1;
  while (keep > 4) {
    const head = Math.ceil(keep / 2);
    const s = `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`;
    if (probe.measureText(s).width <= max) return s;
    keep -= Math.max(1, Math.floor(keep / 16));
  }
  return text;
}

/** Wires the status bar; returns `{update(status), setPath(text), flash(text)}`. */
export function initStatus(app) {
  $('st-elevate').onclick = () => api.elevate(app.tab.id);
  $('st-stale').onclick = () => app.toggleScan();
  const path = $('st-path');
  let full = '';
  const draw = () => {
    const font = getComputedStyle(path).font;
    path.textContent = elide(full, path.clientWidth, font);
  };
  new ResizeObserver(draw).observe(path);
  const msg = $('st-msg');
  let msgTimer = 0;
  return {
    flash: (text) => {
      msg.textContent = text;
      msg.title = text;
      clearTimeout(msgTimer);
      msgTimer = setTimeout(() => (msg.textContent = msg.title = ''), MSG_MS);
    },
    setPath: (text) => {
      full = text;
      draw();
    },
    update: (st) => update(st),
  };
}

function stateWord(st) {
  if (st.running) return 'Scanning';
  if (st.cancelled) return 'Stopped';
  return st.filter_text ? 'Filtering' : 'Ready';
}

const seconds = (ms) => `${(ms / 1000).toFixed(1)} s`;

function update(st) {
  const state = $('st-state');
  state.classList.toggle('scanning', !!st.running);
  $('st-word').textContent = st.active ? stateWord(st) : 'Ready';
  const prog = $('st-prog');
  prog.hidden = !st.running;
  prog.classList.toggle('busy', !st.volume);
  prog.firstElementChild.style.width = st.volume ? `${st.volume.percent}%` : '';
  const pct = st.volume && st.running ? `${st.volume.percent}% · ` : '';
  const totals = () => `${st.files.toLocaleString()} files · ${st.dirs.toLocaleString()} folders · ${st.scanned}`;
  $('st-count').textContent = st.active ? `${pct}${totals()}${st.running ? '' : ` · ${seconds(st.elapsed_ms)}`}` : '';
  const err = $('st-err');
  err.textContent = st.errors ? `${st.errors.toLocaleString()} errors` : '';
  err.title = st.error_samples?.join(String.fromCharCode(10)) ?? '';
  const backend = $('st-backend');
  backend.textContent = st.active ? st.backend : '';
  backend.title = st.backend_tip ?? '';
  const elevate = $('st-elevate');
  elevate.hidden = !st.can_elevate;
  elevate.title = st.backend_tip ?? '';
  $('st-stale').hidden = !st.stale;
  $('st-pct').textContent = st.volume ? `${st.volume.percent}% of ${st.volume.root.replace(/\\$/, '')}` : '';
}
