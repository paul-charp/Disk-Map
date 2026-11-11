// Small form controls for the settings dialog.
import { el } from './dom.js';
import { setIcon } from './icons.js';

/** Labelled row of the settings grid. */
export function row(label, control) {
  const r = el('div', 'srow');
  r.append(el('span', 'slabel', label), control);
  return r;
}

/** Icon-only button with a tooltip. */
export function iconButton(icon, title, run) {
  const b = el('button', 'ib');
  b.title = title;
  b.setAttribute('aria-label', title);
  setIcon(b, icon);
  b.onclick = run;
  return b;
}

/** Text button. */
export function button(label, run) {
  const b = el('button', 'btn', label);
  b.onclick = run;
  return b;
}

/** On/off switch. */
export function toggle(value, onChange) {
  const b = el('button', 'tgl');
  b.setAttribute('role', 'switch');
  b.append(el('i'));
  const set = (v) => {
    b.setAttribute('aria-checked', v);
  };
  set(value);
  b.onclick = () => {
    value = !value;
    set(value);
    onChange(value);
  };
  return b;
}

/** Buttons of which one is selected; `options` are `[value, label]`. */
export function segmented(options, value, onChange) {
  const seg = el('span', 'seg');
  const buttons = options.map(([v, label]) => {
    const b = el('button', '', label);
    b.onclick = () => {
      value = v;
      mark();
      onChange(v);
    };
    seg.append(b);
    return [v, b];
  });
  const mark = () => buttons.forEach(([v, b]) => b.classList.toggle('on', v === value));
  mark();
  return seg;
}

/** Minus, value and plus over a list of allowed values. */
export function stepper(values, value, format, onChange) {
  const box = el('span', 'step');
  const out = el('span', 'sval mono');
  let i = Math.max(0, values.indexOf(value));
  const move = (d) => {
    i = Math.max(0, Math.min(values.length - 1, i + d));
    show();
    onChange(values[i]);
  };
  const minus = iconButton('minus', 'Less', () => move(-1));
  const plus = iconButton('plus', 'More', () => move(1));
  const show = () => {
    out.textContent = format(values[i]);
    minus.disabled = i === 0;
    plus.disabled = i === values.length - 1;
  };
  box.append(minus, out, plus);
  show();
  return box;
}

/** Round colour buttons, one per theme. */
export function swatches(themes, value, onPick) {
  const box = el('span', 'swatches');
  const buttons = themes.map((t) => {
    const b = el('button', 'swatch');
    b.title = t.name;
    b.setAttribute('aria-label', t.name);
    b.style.setProperty('--sw', t.swatch);
    b.onclick = () => {
      value = t.name;
      mark();
      onPick(t.name);
    };
    box.append(b);
    return [t.name, b];
  });
  const mark = () => buttons.forEach(([n, b]) => b.setAttribute('aria-pressed', n === value));
  mark();
  return box;
}
