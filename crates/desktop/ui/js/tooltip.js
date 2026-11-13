// Tooltip following the cursor after a short hover.
import { $, el } from './dom.js';

const DELAY_MS = 500;

function rows(d) {
  const kv = el('div', 'kv');
  const add = (k, v) => v && kv.append(el('span', '', k), el('span', '', v));
  add('Size', d.size);
  add('On disk', d.disk);
  add('Before', d.old);
  add('Change', d.change && `${d.delta} · ${d.change}`);
  add('Modified', d.modified);
  add('Created', d.created);
  add('Accessed', d.accessed);
  add('Items', d.items == null ? '' : d.items.toLocaleString());
  return kv;
}

/** Wires the tooltip; returns `{hover(node), move(e), hide()}`. */
export function initTooltip(app) {
  const tip = $('tip');
  let timer = 0;
  let last = null;
  const place = (e) => {
    const { width, height } = tip.getBoundingClientRect();
    tip.style.left = `${Math.min(e.clientX + 14, innerWidth - width - 4)}px`;
    tip.style.top = `${Math.min(e.clientY + 18, innerHeight - height - 4)}px`;
  };
  const hide = () => {
    clearTimeout(timer);
    tip.hidden = true;
  };
  return {
    hide,
    move: (e) => {
      last = e;
      if (!tip.hidden) place(e);
    },
    hover: (node, e) => {
      hide();
      if (node === null || !e) return;
      last = e;
      timer = setTimeout(async () => {
        const d = await app.details(node);
        tip.replaceChildren(el('b', '', d.name), el('div', 'p', d.path), rows(d));
        tip.hidden = false;
        place(last);
      }, DELAY_MS);
    },
  };
}
