// Small DOM helpers.
export const $ = (id) => document.getElementById(id);

/** Creates an element with a class and optional text. */
export function el(tag, cls = '', text = '') {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (text) e.textContent = text;
  return e;
}

/** Copies text to the clipboard. */
export async function copyText(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const t = el('textarea');
    t.value = text;
    document.body.append(t);
    t.select();
    document.execCommand('copy');
    t.remove();
  }
}
