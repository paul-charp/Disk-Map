// Filter row: text field, parse error and match counts.
import { $ } from './dom.js';
import { openMenu } from './menu.js';

const DEBOUNCE_MS = 300;

const PRESETS = [
  ['Large files', '>1gb'],
  ['Old files', 'modify>1y'],
  ['Recent', 'modify<7d'],
  ['Videos', ':class:Video'],
  ['Archives', ':class:Archives'],
  ['Temp', '*.tmp;*.temp'],
  ['Caches', '\\*cache*'],
  ['node_modules', '\\node_modules'],
  ['Hidden', ':attr:hidden'],
  ['Changed', '|is:same'],
  ['New', 'is:new'],
];

const SYNTAX = [
  ['Size', [['>100mb', 'Allocated'], ['size<1mb', 'Logical']]],
  ['Age', [['modify<7d', 'Modified'], ['access>1y', 'Accessed'], ['creation>2y', 'Created']]],
  ['Name', [['*.ext', 'Mask'], ['\\folder', 'Folder'], ['|', 'Not']]],
  ['Tags', [[':tag:red', 'Tag']]],
  ['Attributes', [[':attr:hidden', 'Hidden'], [':attr:system', 'System']]],
  ['Compare', [['is:new', 'New'], ['is:grown', 'Grown'], ['delta:>1gb', 'Delta']]],
];

/** Text after inserting `token`: `;` separated, except for the `|` prefix. */
export function insertToken(text, token) {
  if (token === '|' || !text || text.endsWith(';')) return text + token;
  return `${text};${token}`;
}

/** Wires the filter field; returns `{update(status), set(text, error), setApply(text), text(), focused()}`. */
export function initFilter(app) {
  const input = $('filter');
  let timer = 0;
  const apply = async () => {
    clearTimeout(timer);
    const info = await app.applyFilter(input.value);
    $('filter-err').textContent = info.error ?? '';
  };
  input.addEventListener('input', () => {
    clearTimeout(timer);
    timer = setTimeout(apply, DEBOUNCE_MS);
  });
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') apply();
    else if (e.key === 'Escape') {
      input.value = '';
      apply();
      input.blur();
    }
  });
  const setApply = (text) => {
    input.value = text;
    input.focus();
    input.setSelectionRange(text.length, text.length);
    apply();
  };
  const anchor = (id) => {
    const r = $(id).getBoundingClientRect();
    return [r.left, r.bottom + 2];
  };
  const put = (token) => setApply(insertToken(input.value, token));
  const classGroup = () => ['Class', (app.cfg.classes ?? []).map((c) => [`:class:${c.name}`, c.name, c.color])];

  const presetsMenu = () => {
    const at = anchor('b-fpresets');
    const saved = app.cfg.filters ?? [];
    const text = input.value.trim();
    const unsave = (f) => async () => {
      await app.patchConfig({ filters: saved.filter((s) => s !== f) });
      presetsMenu();
    };
    const items = [];
    if (saved.length) {
      items.push({ title: 'Saved' }, ...saved.map((f) => ({ label: f, run: () => setApply(f), remove: unsave(f) })));
    }
    items.push({ title: 'Presets' }, ...PRESETS.map(([label, f]) => ({ label, hint: f, run: () => setApply(f) })), { sep: true });
    if (text && !saved.includes(text)) items.push({ label: 'Save', run: () => app.patchConfig({ filters: [...saved, text] }) });
    items.push({ label: 'Clear', run: () => setApply('') });
    openMenu(items, ...at);
  };

  const syntaxMenu = () => {
    const groups = [...SYNTAX.slice(0, -1), classGroup(), SYNTAX.at(-1)].filter(([, rows]) => rows.length);
    const items = groups.flatMap(([title, rows]) => [
      { title },
      ...rows.map(([token, label, swatch]) => ({ label, hint: token, swatch, run: () => put(token) })),
    ]);
    openMenu(items, ...anchor('b-fhelp'));
  };
  $('b-fpresets').onclick = presetsMenu;
  $('b-fhelp').onclick = syntaxMenu;

  return {
    update: (st) => {
      $('filter-count').textContent = st.active ? `${st.matched.toLocaleString()} / ${st.files.toLocaleString()}` : '';
    },
    set: (text, error) => {
      clearTimeout(timer);
      input.value = text;
      $('filter-err').textContent = error;
    },
    setApply,
    text: () => input.value,
    focused: () => document.activeElement === input,
  };
}
