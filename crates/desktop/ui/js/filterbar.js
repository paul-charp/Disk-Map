// Filter row: text field, parse error and match counts.
import { $ } from './dom.js';

const DEBOUNCE_MS = 300;

/** Wires the filter field; returns `{update(status), set(text, error), text(), focused()}`. */
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
  return {
    update: (st) => {
      $('filter-count').textContent = st.active ? `${st.matched.toLocaleString()} / ${st.files.toLocaleString()}` : '';
    },
    set: (text, error) => {
      clearTimeout(timer);
      input.value = text;
      $('filter-err').textContent = error;
    },
    text: () => input.value,
    focused: () => document.activeElement === input,
  };
}
