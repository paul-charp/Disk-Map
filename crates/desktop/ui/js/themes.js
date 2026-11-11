// Chrome themes; the colours live in style.css.
export const THEMES = [
  { name: 'graphite', swatch: '#4c9aff' },
  { name: 'paper', swatch: '#0b63ce' },
  { name: 'dusk', swatch: '#62a8f0' },
  { name: 'ember', swatch: '#d9825b' },
  { name: 'violet', swatch: '#a78bfa' },
];

/** Switches the chrome theme. */
export function applyTheme(name) {
  document.documentElement.dataset.theme = name;
}
