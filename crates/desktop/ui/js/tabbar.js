// Tab strip: one button per tab with scan indicator and close button, plus the new-tab button.
import { $, el } from './dom.js';
import { setIcon } from './icons.js';
import { titleOf } from './tabs.js';

/** Wires the strip; returns `{render(), update(tab)}`. */
export function initTabbar(app) {
  const strip = $('tabs');
  const rows = new Map();

  const build = (tab) => {
    const e = el('div', 'tab');
    e.setAttribute('role', 'tab');
    const spin = el('i', 'spin');
    const name = el('span', 'tname');
    const close = el('button', 'tclose');
    setIcon(close, 'close');
    close.title = 'Close (Ctrl+W)';
    close.onclick = (ev) => {
      ev.stopPropagation();
      app.closeTab(tab.id);
    };
    e.append(spin, name, close);
    e.onmousedown = (ev) => {
      if (ev.button === 1) ev.preventDefault();
    };
    e.onauxclick = (ev) => ev.button === 1 && app.closeTab(tab.id);
    e.onclick = () => app.activate(tab.id);
    rows.set(tab.id, { e, name });
    return e;
  };

  const paint = (tab) => {
    const r = rows.get(tab.id);
    r.name.textContent = titleOf(tab.path, app.driveLabels);
    r.e.title = tab.path || 'New tab';
    r.e.classList.toggle('active', tab === app.tab);
    r.e.classList.toggle('scanning', !!tab.status.running);
    r.e.setAttribute('aria-selected', tab === app.tab);
  };

  $('b-newtab').onclick = () => app.newTab();
  strip.addEventListener('wheel', (e) => {
    strip.scrollLeft += e.deltaY || e.deltaX;
  });

  return {
    render: () => {
      for (const id of rows.keys()) if (!app.tabs.some((t) => t.id === id)) rows.delete(id);
      strip.replaceChildren(...app.tabs.map((t) => rows.get(t.id)?.e ?? build(t)));
      app.tabs.forEach(paint);
      rows.get(app.tab.id).e.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    },
    update: (tab) => rows.has(tab.id) && paint(tab),
  };
}
