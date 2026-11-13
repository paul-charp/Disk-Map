// Entry point: wires the backend, the map and the chrome together.
import { app, TEXT_SCALES } from './app.js';
import { api, onDrop, onScan } from './api.js';
import { $, copyText } from './dom.js';
import { initFilter } from './filterbar.js';
import { applyScanFlags } from './flags.js';
import { mountIcons } from './icons.js';
import { initKeys } from './keys.js';
import { MapView } from './map.js';
import { bindMenuDismiss, closeMenu, confirmMenu, openMenu } from './menu.js';
import { initOutput } from './output.js';
import { initSettings } from './settings.js';
import { initSide } from './side.js';
import { initStart } from './start.js';
import { initStatus } from './statusbar.js';
import { initTabbar } from './tabbar.js';
import { applyTheme } from './themes.js';
import { initToolbar } from './toolbar.js';
import { initTooltip } from './tooltip.js';

const HOVER_PATH_MS = 60;

/** Items acting on a real file or folder. */
function nodeItems(node, d, e) {
  const folder = d.kind === 'folder';
  return [
    { label: 'Open', run: () => app.open(node) },
    { label: 'Open in terminal', run: () => app.terminal(node) },
    { label: 'Reveal', run: () => app.reveal(node) },
    { label: 'Copy path', run: () => copyText(d.path) },
    { label: 'Properties', run: () => app.properties(node) },
    { sep: true },
    ...(folder
      ? [
          { label: 'Scan in new tab', run: () => app.newTab(d.path) },
          { label: d.pinned ? 'Unpin' : 'Pin', run: () => api.setPin(d.path, !d.pinned) },
        ]
      : []),
    { label: 'Recycle', run: () => confirmMenu(d.name, 'Recycle', () => app.recycle(node), e.clientX, e.clientY) },
    { sep: true },
    { tags: [1, 2, 3, 4].map((i) => ({ on: d.tag === i, run: (n) => app.tag(n, node) })) },
    { sep: true },
  ];
}

function contextMenu(node, e) {
  closeMenu();
  if (node === null) return;
  app.details(node).then((d) => {
    const real = d.kind === 'file' || d.kind === 'folder';
    const items = real ? nodeItems(node, d, e) : [];
    items.push({ label: 'Zoom', run: () => app.zoom(node) });
    openMenu(items, e.clientX, e.clientY);
  });
}

function mapHandlers(tooltip) {
  let pathTimer = 0;
  return {
    onHover: (node, e) => {
      app.hovered = node !== null && node >= 0 ? node : null;
      tooltip.hover(node, e);
      clearTimeout(pathTimer);
      pathTimer = setTimeout(app.showPath, HOVER_PATH_MS);
    },
    onMove: (e) => tooltip.move(e),
    onLeave: () => tooltip.hide(),
    onSelect: (node) => app.select(node),
    onZoom: (node) => app.zoom(node),
    onMenu: contextMenu,
    onBack: () => app.status.can_back && app.nav('back'),
    onForward: () => app.status.can_fwd && app.nav('forward'),
  };
}

function perfLine(p) {
  const f = (v) => v.toFixed(1);
  return `items=${p.items} layout=${f(p.backend)}ms roundtrip=${f(p.trip)}ms decode=${f(p.decode)}ms paint=${f(p.paint)}ms decode+paint=${f(p.decode + p.paint)}ms`;
}

/** Waits for a frame to be painted, then shows the window so the first thing seen is already themed. */
async function showWhenPainted() {
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  await api.showWindow();
}

async function loadDrives() {
  const info = await api.startInfo().catch(() => null);
  if (!info) return;
  app.driveLabels = new Map(info.drives.map((d) => [d.root, d.label]));
  app.parts.tabbar.render();
}

async function main() {
  mountIcons();
  const boot = await api.boot();
  app.cfg = boot.config;
  app.classes = boot.classes;
  app.defaultClasses = boot.default_classes;
  applyTheme(app.cfg.theme);
  const tooltip = initTooltip(app);
  app.parts = {
    tooltip,
    tabbar: initTabbar(app),
    toolbar: initToolbar(app),
    filter: initFilter(app),
    statusbar: initStatus(app),
    side: initSide(app),
    start: initStart(app),
    settings: initSettings(app),
    output: await initOutput(),
  };
  app.map = new MapView($('map'), {
    classes: app.classes,
    scheme: app.scheme,
    textScale: TEXT_SCALES[app.cfg.map_text],
    animate: app.cfg.animation,
    handlers: mapHandlers(tooltip),
    onPerf: (p) => {
      const line = perfLine(p);
      if (boot.debug) console.debug(line);
      api.perf(line);
    },
  });
  initKeys(app);
  bindMenuDismiss();
  document.addEventListener('contextmenu', (e) => e.preventDefault());
  await openStartTabs(boot);
  await showWhenPainted();
  loadDrives();
  if (boot.flags.log) app.parts.output.show();
  if (boot.flags.settings) app.parts.settings.open(boot.flags.settings);
}

/** Opens the tabs from the command line, or one start tab, and wires scan events and drops. */
async function openStartTabs(boot) {
  const last = boot.tabs.length - 1;
  let pending = last >= 0 ? { index: last, flags: boot.flags } : null;
  await onScan((st) => {
    if (app.setStatus(st) && st.tab === app.tab.id) app.map.request();
    const tab = app.tabs[pending?.index];
    if (tab?.id === st.tab && st.active && !st.running) {
      const { flags } = pending;
      pending = null;
      applyScanFlags(app, tab, flags);
    }
  });
  await onDrop((paths) => app.openPaths(paths));
  $('app').hidden = false;
  if (last < 0) return app.newTab();
  for (const [i, p] of boot.tabs.entries()) await app.newTab(p, i === last);
}

/** Sends an uncaught script error to the log. */
const report = (message) => api.logJs('error', message).catch(() => {});
addEventListener('error', (e) => report(`${e.error?.stack ?? e.message} (${e.filename}:${e.lineno})`));
// Command errors reject with their text and are already logged by the backend.
addEventListener('unhandledrejection', (e) => typeof e.reason !== 'string' && report(`Unhandled: ${e.reason?.stack ?? e.reason}`));
main();
