// Tab strip: one button per tab with scan indicator and close button, plus the new-tab button.
import { $, el } from './dom.js';
import { setIcon } from './icons.js';
import { titleOf } from './tabs.js';

/** Wires the strip; returns `{render(), update(tab)}`. */
export function initTabbar(app) {
  const strip = $('tabs');
  const rows = new Map();

  const DRAG_PX = 4;
  let dragged = false;

  /** Swaps the dragged element with the neighbours whose middle it has crossed. */
  const reorder = (e, left) => {
    const mid = () => left() + e.offsetWidth / 2;
    const middle = (n) => n.offsetLeft + n.offsetWidth / 2;
    for (let p = e.previousElementSibling; p && mid() < middle(p); p = e.previousElementSibling) strip.insertBefore(e, p);
    for (let n = e.nextElementSibling; n && mid() > middle(n); n = e.nextElementSibling) strip.insertBefore(n, e);
  };

  /** Drags a tab sideways once the pointer moved a few pixels; the order is committed on release. */
  const startDrag = (ev, tab, e) => {
    if (ev.button !== 0 || ev.target.closest('.tclose')) return;
    const x0 = ev.clientX;
    const left0 = e.offsetLeft;
    let on = false;
    const place = (x) => {
      const off = () => x - x0 - (e.offsetLeft - left0);
      e.style.transform = `translateX(${off()}px)`;
      reorder(e, () => e.offsetLeft + off());
      e.style.transform = `translateX(${off()}px)`;
    };
    const move = (m) => {
      if (!on && Math.abs(m.clientX - x0) < DRAG_PX) return;
      if (!on) {
        on = true;
        dragged = true;
        e.classList.add('dragging');
      }
      place(m.clientX);
    };
    const up = () => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      removeEventListener('pointercancel', up);
      if (!on) return;
      e.style.transform = '';
      e.classList.remove('dragging');
      setTimeout(() => (dragged = false));
      const index = [...strip.children].indexOf(e);
      if (index === app.tabs.indexOf(tab)) return;
      app.moveTab(tab.id, index);
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
    addEventListener('pointercancel', up);
  };

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
    e.onclick = () => !dragged && app.activate(tab.id);
    e.onpointerdown = (ev) => startDrag(ev, tab, e);
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
