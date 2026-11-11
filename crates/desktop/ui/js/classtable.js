// Settings section for the file classes: an editable table of name, colour and extensions.
import { el } from './dom.js';
import { button, iconButton } from './controls.js';

const COMMIT_MS = 250;

const toText = (c) => c.extensions.join(';');
const parseExt = (text) => text.split(';').map((e) => e.trim()).filter(Boolean);

function input(cls, value, label, onInput) {
  const i = el('input', cls);
  i.type = cls === 'ccolor' ? 'color' : 'text';
  i.value = value;
  i.spellcheck = false;
  i.setAttribute('aria-label', label);
  i.oninput = () => onInput(i.value);
  return i;
}

/** Builds the classes section; edits are applied through `app.patchConfig`. */
export function classesSection(app) {
  let rows = app.cfg.classes.map((c) => ({ ...c, extensions: [...c.extensions] }));
  let timer = 0;
  const box = el('div', 'classes');
  const head = el('div', 'crow chead');
  head.append(el('span', '', 'Name'), el('span', '', 'Colour'), el('span', '', 'Extensions'), el('span'));
  const list = el('div', 'clist');
  const foot = el('div', 'cfoot');

  const commit = (now = false) => {
    clearTimeout(timer);
    const send = () => app.patchConfig({ classes: rows.map((c) => ({ ...c, extensions: [...c.extensions] })) });
    if (now) send();
    else timer = setTimeout(send, COMMIT_MS);
  };
  const change = (fn) => {
    fn();
    draw();
    commit(true);
  };
  const move = (i, d) => change(() => rows.splice(i + d, 0, ...rows.splice(i, 1)));

  const rowEl = (c, i) => {
    const r = el('div', 'crow');
    const actions = el('span', 'cact');
    const up = iconButton('up', 'Up', () => move(i, -1));
    const down = iconButton('down', 'Down', () => move(i, 1));
    up.disabled = i === 0;
    down.disabled = i === rows.length - 1;
    actions.append(up, down, iconButton('trash', 'Delete', () => change(() => rows.splice(i, 1))));
    r.append(
      input('cname', c.name, 'Name', (v) => { c.name = v; commit(); }),
      input('ccolor', c.color, 'Colour', (v) => { c.color = v; commit(); }),
      input('cext mono', toText(c), 'Extensions', (v) => { c.extensions = parseExt(v); commit(); }),
      actions,
    );
    return r;
  };

  const draw = () => list.replaceChildren(...rows.map(rowEl));
  foot.append(
    button('Add', () => change(() => rows.push({ name: 'Class', color: '#868e96', extensions: [] }))),
    button('Reset', () => change(() => { rows = app.defaultClasses.map((c) => ({ ...c, extensions: [...c.extensions] })); })),
  );
  draw();
  box.append(head, list, foot);
  return box;
}
