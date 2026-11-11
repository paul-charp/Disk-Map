// Start screen of a tab: path field, drives and recent scans.
import { api } from './api.js';
import { $, el } from './dom.js';

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

function recentRow(r, scan) {
  const row = el('div', 'item recent');
  row.append(el('span', 't', r.path), el('span', 's', r.size));
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
      const info = await api.startInfo();
      app.driveLabels = new Map(info.drives.map((d) => [d.root, d.label]));
      app.parts.tabbar.render();
      $('start-drives').replaceChildren(...info.drives.map((d) => driveRow(d, scan)));
      $('start-recent').replaceChildren(...info.recent.map((r) => recentRow(r, scan)));
    },
  };
}
