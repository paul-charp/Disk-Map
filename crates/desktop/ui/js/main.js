// Entry point: wires the backend, the map and the chrome together.
import { app, TEXT_SCALES } from './app.js';
import { api, onDrop, onOpenTabs, onScan } from './api.js';
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
import { initUpdate } from './update.js';

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
    { label: 'Export CSV', run: () => app.exportTo('selection', 'csv', node) },
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
  app.parts.update = initUpdate(app);
  if (boot.flags.log) app.parts.output.show();
  if (boot.flags.settings) app.parts.settings.open(boot.flags.settings);
}

/** Opens Home and the tabs from the command line (an empty entry is Home), and wires scan events and drops. */
async function openStartTabs(boot) {
  await onScan((st) => {
    if (app.setStatus(st) && st.tab === app.tab.id) app.map.request();
    app.scanEvent(st);
  });
  await onDrop((paths) => app.openPaths(paths));
  await onOpenTabs((paths) => app.openPaths(paths));
  $('app').hidden = false;
  await app.activate(app.home().id);
  const last = boot.tabs.length - 1;
  for (const [i, p] of boot.tabs.entries()) {
    if (!p) {
      if (i === last) await app.activate(app.home().id);
      continue;
    }
    const tab = app.addTab(i === last);
    if (i === last) app.afterScan(tab, () => applyScanFlags(app, tab, boot.flags));
    await app.scan(tab, p);
  }
}

/** Sends an uncaught script error to the log. */
const report = (message) => api.logJs('error', message).catch(() => {});
addEventListener('error', (e) => report(`${e.error?.stack ?? e.message} (${e.filename}:${e.lineno})`));
// Command errors reject with their text and are already logged by the backend.
addEventListener('unhandledrejection', (e) => typeof e.reason !== 'string' && report(`Unhandled: ${e.reason?.stack ?? e.reason}`));
main();
