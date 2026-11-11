// Popup menu at a screen position.
import { $, el } from './dom.js';

/** Colours of the four tags. */
export const TAG_COLORS = ['#e03131', '#f2c230', '#2f9e44', '#1c7ed6'];

/** Hides the menu. */
export function closeMenu() {
  $('menu').hidden = true;
}

/**
 * Shows a menu. Items: `{label, run, swatch?}`, `{sep: true}` or `{tags: [{on, run}, ...]}`.
 */
export function openMenu(items, x, y) {
  const m = $('menu');
  m.replaceChildren(...items.map(row));
  m.hidden = false;
  const { width, height } = m.getBoundingClientRect();
  m.style.left = `${Math.max(0, Math.min(x, innerWidth - width - 2))}px`;
  m.style.top = `${Math.max(0, Math.min(y, innerHeight - height - 2))}px`;
}

function row(item) {
  if (item.sep) return el('hr');
  if (item.tags) return tagRow(item.tags);
  const b = el('button');
  if (item.swatch) {
    const sw = el('span', 'sw');
    sw.style.background = item.swatch;
    b.append(sw);
  }
  b.append(item.label);
  b.onclick = () => {
    closeMenu();
    item.run();
  };
  return b;
}

function tagRow(tags) {
  const r = el('div', 'tags');
  tags.forEach((t, i) => {
    const d = el('span', `dot${t.on ? ' on' : ''}`);
    d.style.background = TAG_COLORS[i];
    d.onclick = () => {
      closeMenu();
      t.run(i + 1);
    };
    r.append(d);
  });
  return r;
}

/** Closes the menu on any outside click, Escape or blur. */
export function bindMenuDismiss() {
  addEventListener('mousedown', (e) => !e.target.closest('#menu') && closeMenu());
  addEventListener('keydown', (e) => e.key === 'Escape' && closeMenu());
  addEventListener('blur', closeMenu);
}
