// Keyboard shortcuts.
const PLUS = new Set(['+', '=', 'Add']);
const MINUS = new Set(['-', '_', 'Subtract']);

/** Shortcut and action pairs, listed in the settings. */
export const SHORTCUTS = [
  ['Ctrl+N', 'New tab'],
  ['Ctrl+W', 'Close tab'],
  ['Ctrl+Tab', 'Next tab'],
  ['Ctrl+Shift+Tab', 'Previous tab'],
  ['Ctrl+Shift+PageUp / PageDown', 'Move tab'],
  ['Ctrl+,', 'Settings'],
  ['F5', 'Rescan / stop'],
  ['Backspace', 'Back'],
  ['Shift+Backspace', 'Forward'],
  ['Ctrl+Up', 'Up'],
  ['Ctrl+Home', 'Root'],
  ['Ctrl++ / Ctrl+-', 'Detail'],
  ['Ctrl+T', 'File colours'],
  ['Ctrl+F', 'Free space'],
  ['Ctrl+U', 'Unknown space'],
  ['Ctrl+L', 'Watch'],
  ['Ctrl+B', 'Side panel'],
  ['Ctrl+`', 'Output'],
  ['Ctrl+1 to 4', 'Tag'],
  ['Ctrl+0', 'Clear tags'],
];

/** Installs the global key handler. */
export function initKeys(app) {
  addEventListener('keydown', (e) => {
    if (e.ctrlKey && e.key === ',') return handled(e, () => app.parts.settings.toggle());
    if (app.parts.settings.isOpen()) return;
    const inField = e.target instanceof HTMLInputElement;
    if (e.ctrlKey && !e.altKey) return ctrlKey(app, e);
    if (app.tab.mode !== 'map') return;
    if (e.key === 'F5') return handled(e, () => app.toggleScan());
    if (inField || app.parts.filter.focused()) return;
    if (e.key === 'Backspace') handled(e, () => app.nav(e.shiftKey ? 'forward' : 'back'));
  });
}

function handled(e, run) {
  e.preventDefault();
  run();
}

function tabKey(app, e) {
  const k = e.key.toLowerCase();
  if (k === 'n') return () => app.newTab();
  if (k === 'w') return () => app.closeTab(app.tab.id);
  if (e.shiftKey && (e.key === 'PageUp' || e.key === 'PageDown')) {
    return () => app.moveTab(app.tab.id, app.tabs.indexOf(app.tab) + (e.key === 'PageUp' ? -1 : 1));
  }
  if (e.key === 'Tab') return () => app.cycle(e.shiftKey ? -1 : 1);
  return null;
}

function ctrlKey(app, e) {
  const tabRun = tabKey(app, e);
  if (tabRun) return handled(e, tabRun);
  if (e.code === 'Backquote' || e.key === '`') return handled(e, () => app.parts.output.toggle());
  if (app.tab.mode !== 'map') return;
  const k = e.key;
  if (PLUS.has(k)) return handled(e, () => app.changeDetail(1));
  if (MINUS.has(k)) return handled(e, () => app.changeDetail(-1));
  const map = {
    t: () => app.cycleFileColors(),
    f: () => app.toggle('free'),
    u: () => app.toggle('unknown'),
    l: () => app.toggle('watch'),
    b: () => app.toggle('side'),
    0: () => app.clearTags(),
    ArrowUp: () => app.nav('up'),
    Home: () => app.nav('home'),
  };
  const run = map[k.toLowerCase()] ?? (k >= '1' && k <= '4' ? () => app.tag(Number(k)) : null);
  if (run) handled(e, run);
}
