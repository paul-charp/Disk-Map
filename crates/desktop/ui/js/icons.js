// Inline stroke icons on a 16x16 grid.
const PATHS = {
  plus: 'M8 3v10M3 8h10',
  minus: 'M3 8h10',
  close: 'M4 4l8 8M12 4l-8 8',
  down: 'M8 3v10M4 9l4 4 4-4',
  trash: 'M3 4.5h10M6.5 4.5V3h3v1.5M4.5 4.5l.6 8.5h5.8l.6-8.5M7 7v4M9 7v4',
  reset: 'M3.5 3.5v3h3M3.8 6.5A5 5 0 1 1 3.2 9',
  gear: 'M6.6 3.3L6.9 1.5L9.1 1.5L9.4 3.3L10.3 3.7L11.8 2.6L13.4 4.2L12.3 5.7L12.7 6.6L14.5 6.9L14.5 9.1L12.7 9.4L12.3 10.3L13.4 11.8L11.8 13.4L10.3 12.3L9.4 12.7L9.1 14.5L6.9 14.5L6.6 12.7L5.7 12.3L4.2 13.4L2.6 11.8L3.7 10.3L3.3 9.4L1.5 9.1L1.5 6.9L3.3 6.6L3.7 5.7L2.6 4.2L4.2 2.6L5.7 3.7zM10.2 8a2.2 2.2 0 1 1-4.4 0 2.2 2.2 0 0 1 4.4 0z',
  rescanView: 'M5 3H3v2M11 3h2v2M5 13H3v-2M11 13h2v-2M10.2 6.6A2.8 2.8 0 0 0 5.4 7.2M5.8 9.4a2.8 2.8 0 0 0 4.8-.6',
  back: 'M13 8H3M7 4 3 8l4 4',
  forward: 'M3 8h10M9 4l4 4-4 4',
  up: 'M8 13V3M4 7l4-4 4 4',
  home: 'M2.5 8 8 3l5.5 5M4 7v6h3V9.5h2V13h3V7',
  rescan: 'M13 3.5V7H9.5M3 12.5V9h3.5M12.6 7A5 5 0 0 0 4 5.4M3.4 9A5 5 0 0 0 12 10.6',
  stop: 'M4.5 4.5h7v7h-7z',
  less: 'M2.5 2.5h5v5h-5zM8.5 2.5h5v5h-5zM2.5 8.5h5v5h-5zM8.5 8.5h5v5h-5z',
  more: 'M2.5 2.5h3v3h-3zM6.5 2.5h3v3h-3zM10.5 2.5h3v3h-3zM2.5 6.5h3v3h-3zM6.5 6.5h3v3h-3zM10.5 6.5h3v3h-3zM2.5 10.5h3v3h-3zM6.5 10.5h3v3h-3zM10.5 10.5h3v3h-3z',
  palette: 'M8 2.5a5.5 5.5 0 1 0 0 11c1 0 1.2-.8.8-1.4-.5-.8 0-1.6.9-1.6H11a2.5 2.5 0 0 0 2.5-2.5C13.5 4.8 11 2.5 8 2.5zM5.5 7.5h.01M8 5.5h.01M10.5 7h.01',
  pie: 'M8 2.5A5.5 5.5 0 1 0 13.5 8H8zM9.5 1.5V6.5h5A5 5 0 0 0 9.5 1.5z',
  unknown: 'M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11zM6.3 6.3a1.8 1.8 0 1 1 2.5 1.7c-.5.3-.8.6-.8 1.2M8 11.3h.01',
  eye: 'M1.5 8S4 3.5 8 3.5 14.5 8 14.5 8 12 12.5 8 12.5 1.5 8 1.5 8zM8 6a2 2 0 1 0 0 4 2 2 0 0 0 0-4z',
  side: 'M2.5 3.5h11v9h-11zM10 3.5v9',
  filter: 'M2.5 3.5h11L9.5 8.5V13l-3-1.5V8.5z',
  copy: 'M5.5 5.5h7v7h-7zM3.5 10.5v-7h7',
  info: 'M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11zM8 7.3v3.7M8 5.2h.01',
  folder: 'M2 4.5h4l1.2 1.5H14v6.5H2z',
  error: 'M8 2.5a5.5 5.5 0 1 0 0 11 5.5 5.5 0 0 0 0-11zM6 6l4 4M10 6l-4 4',
  warning: 'M8 2.2 14.2 13H1.8zM8 6.5v3M8 11.3h.01',
};

/** Fills every `svg[data-icon]` and replaces `button[data-icon]` content with its icon. */
export function mountIcons(root = document) {
  for (const el of root.querySelectorAll('[data-icon]')) setIcon(el, el.dataset.icon);
}

/** Puts the named icon into an element (an svg, or a button that gets an svg child). */
export function setIcon(el, name) {
  const svg = el instanceof SVGElement ? el : document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('viewBox', '0 0 16 16');
  svg.innerHTML = `<path d="${PATHS[name]}"/>`;
  if (svg !== el) el.replaceChildren(svg);
}
