// Start screen of a tab: path field, drives, pinned folders and recent scans.
import { api } from './api.js';
import { $, el } from './dom.js';
import { setIcon } from './icons.js';

function driveRow(d, scan) {
  const row = el('div', 'item');
  const bar = el('div', 'u');
  const fill = el('i');
  fill.style.width = `${(d.share * 100).toFixed(1)}%`;
  bar.append(fill);
  row.append(el('span', 'l', d.root.replace(/\\$/, '')), el('span', 't', d.label), el('span', 'k', d.kind), bar, el('span', 's', d.usage));
  row.onclick = () => scan(d.root);
  return row;
}

function pinButton(r, refresh) {
  const b = el('button', r.pinned ? 'ib pin on' : 'ib pin');
  b.title = r.pinned ? 'Unpin' : 'Pin';
  setIcon(b, 'pin');
  b.onclick = async (e) => {
    e.stopPropagation();
    await api.setPin(r.path, !r.pinned);
    refresh();
  };
  return b;
}

function pathRow(r, scan, refresh) {
  const row = el('div', 'item recent');
  row.append(el('span', 't', r.path), el('span', 's', r.size), pinButton(r, refresh));
  row.onclick = () => scan(r.path);
  return row;
}

/** Wires the start screen; returns `{show(tab), text(), error(text)}`. */
export function initStart(app) {
  const input = $('start-path');
  /** Scans the first of the `;`-separated paths in this tab and opens the others in new tabs. */
  const scan = async (text) => {
    const [first, ...rest] = text.split(';').map((p) => p.trim()).filter(Boolean);
    if (!first) return;
    const tab = app.tab;
    for (const p of rest) await app.newTab(p, false);
    await app.scan(tab, first);
  };
  const refresh = async () => {
    const info = await api.startInfo();
    app.driveLabels = new Map(info.drives.map((d) => [d.root, d.label]));
    app.parts.tabbar.render();
    $('start-drives').replaceChildren(...info.drives.map((d) => driveRow(d, scan)));
    $('start-pinned').hidden = info.pins.length === 0;
    $('start-pins').replaceChildren(...info.pins.map((r) => pathRow(r, scan, refresh)));
    $('start-recent').replaceChildren(...info.recent.map((r) => pathRow(r, scan, refresh)));
  };
  $('start-go').onclick = () => scan(input.value);
  $('start-browse').onclick = async () => {
    const picked = await api.browse();
    if (picked) scan(picked);
  };
  input.addEventListener('keydown', (e) => e.key === 'Enter' && scan(input.value));
  return {
    text: () => input.value,
    error: (text) => {
      $('start-err').textContent = text;
    },
    show: async (tab) => {
      input.value = tab.startText;
      $('start-err').textContent = tab.startError;
      input.focus();
      await refresh();
    },
  };
}
