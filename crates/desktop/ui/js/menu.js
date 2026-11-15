// Popup menu at a screen position.
import { $, el } from './dom.js';
import { setIcon } from './icons.js';

/** Colours of the four tags. */
export const TAG_COLORS = ['#e03131', '#f2c230', '#2f9e44', '#1c7ed6'];

/** Hides the menu. */
export function closeMenu() {
  $('menu').hidden = true;
}

/**
 * Shows a menu. Items: `{label, run, swatch?, danger?, on?, hint?, remove?}`, `{title}`, `{sep: true}` or `{tags: [{on, run}, ...]}`.
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
  if (item.title) return titleRow(item.title);
  const b = el('button', [item.danger && 'danger', item.on && 'on'].filter(Boolean).join(' '));
  if (item.swatch) {
    const sw = el('span', 'sw');
    sw.style.background = item.swatch;
    b.append(sw);
  }
  b.append(item.label);
  if (item.hint) b.append(el('span', 'hint', item.hint));
  b.onclick = () => {
    closeMenu();
    item.run();
  };
  return item.remove ? removable(b, item.remove) : b;
}

/** Wraps a row with a small button that runs `remove` and keeps the menu open. */
function removable(b, remove) {
  const w = el('div', 'item');
  const x = el('button', 'rm');
  setIcon(x, 'close');
  x.title = 'Remove';
  x.onclick = () => remove();
  w.append(b, x);
  return w;
}

function titleRow(text) {
  const t = el('div', 'title');
  t.textContent = text;
  t.title = text;
  return t;
}

/** Asks before an action: a title row, the action and Cancel at the same spot. */
export function confirmMenu(title, label, run, x, y) {
  openMenu([{ title }, { sep: true }, { label, run, danger: true }, { label: 'Cancel', run: () => {} }], x, y);
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
