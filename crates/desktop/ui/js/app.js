// Application state and the actions the chrome calls: tabs, navigation, filter and configuration.
import { api } from './api.js';
import { $ } from './dom.js';
import { closeMenu } from './menu.js';
import { SCHEMES } from './scheme.js';
import { newTab } from './tabs.js';
import { applyTheme } from './themes.js';

const TOGGLE_KEYS = { free: 'show_free', unknown: 'show_unknown' };
/** File colour modes with their labels, in cycle order. */
export const FILE_COLORS = [['accent', 'Accent'], ['class', 'Class'], ['age', 'Age']];
const DETAIL_MAX = 12;
export const TEXT_SCALES = { small: 0.9, medium: 1, large: 1.2 };

export const app = {
  cfg: null,
  classes: [],
  defaultClasses: [],
  classVer: 0,
  driveLabels: new Map(),
  tabs: [],
  tab: null,
  hovered: null,
  parts: {},
  get status() {
    return this.tab.status;
  },
  get scheme() {
    return SCHEMES[this.cfg.theme] ?? SCHEMES.graphite;
  },
};

const tabOf = (id) => app.tabs.find((t) => t.id === id);

app.details = (node) => {
  const { id, detailCache } = app.tab;
  if (!detailCache.has(node)) detailCache.set(node, api.details(id, node));
  return detailCache.get(node);
};

/** True when a snapshot is older than the one already held, e.g. a command answer overtaken by scan events. */
const isStale = (st, cur) => cur.active && (st.epoch < cur.epoch || (st.epoch === cur.epoch && st.ver < cur.ver));

/** Stores a status snapshot in its tab and refreshes the chrome when that tab is active; true when the layout changed. */
app.setStatus = (st) => {
  const tab = tabOf(st.tab);
  if (!tab || isStale(st, tab.status)) return false;
  const changed = st.ver !== tab.status.ver || st.active !== tab.status.active;
  tab.status = st;
  if (changed) tab.detailCache.clear();
  app.parts.tabbar.update(tab);
  if (tab === app.tab) app.refreshChrome();
  return changed;
};

app.refreshChrome = () => {
  const { toolbar, filter, statusbar, side } = app.parts;
  const st = app.status;
  toolbar(st);
  filter.update(st);
  statusbar.update(st);
  side.update(st, app.tab.id);
  app.showPath();
};

app.act = async (promise, animate = false) => {
  app.setStatus(await promise);
  await app.map.refresh({ animate });
};

app.select = (node) => app.act(api.select(app.tab.id, node));
app.zoom = (node) => app.act(api.zoom(app.tab.id, node), true);
app.nav = (kind) => app.act(api[kind](app.tab.id), true);
app.tag = (n, node = app.hovered ?? app.status.selection) => node != null && node >= 0 && app.act(api.tag(app.tab.id, node, n));
app.clearTags = () => app.act(api.clearTags(app.tab.id));
app.reveal = (node) => api.reveal(app.tab.id, node);
app.open = (node) => api.open(app.tab.id, node);

app.applyFilter = async (text) => {
  const tab = app.tab;
  tab.filterText = text;
  const info = await api.setFilter(tab.id, text);
  tab.filterError = info.error ?? '';
  app.setStatus(await api.status(tab.id));
  app.map.refresh();
  return info;
};

app.toggleScan = async () => {
  if (!app.status.active) return;
  if (app.status.running) return app.setStatus(await api.stop(app.tab.id));
  await app.act(api.rescan(app.tab.id));
};

app.rescanView = async () => app.setStatus(await api.rescanView(app.tab.id));

app.changeDetail = async (delta) => {
  const detail = Math.max(1, Math.min(DETAIL_MAX, app.status.detail + delta));
  if (detail !== app.status.detail) await app.act(api.setDetail(app.tab.id, detail));
};

app.toggle = async (kind) => {
  if (kind === 'side') return app.patchConfig({ show_side: !app.cfg.show_side });
  if (kind === 'watch') return app.patchConfig({ watch: !app.cfg.watch });
  await app.act(api.setToggle(app.tab.id, kind, !app.status[TOGGLE_KEYS[kind]]));
};

app.setFileColors = (mode) => app.act(api.setFileColors(app.tab.id, mode));

/** Steps to the next file colour mode. */
app.cycleFileColors = () => {
  const i = FILE_COLORS.findIndex(([m]) => m === app.status.file_colors);
  return app.setFileColors(FILE_COLORS[(i + 1) % FILE_COLORS.length][0]);
};

// ----- configuration -----

/** Applies the parts of a configuration change that live in the front end. */
function applyLocal(prev) {
  const c = app.cfg;
  if (c.theme !== prev.theme) {
    applyTheme(c.theme);
    app.map.setScheme(app.scheme);
  }
  app.map.animate = c.animation;
  if (c.map_text !== prev.map_text) app.map.setText(TEXT_SCALES[c.map_text]);
  if (c.show_side !== prev.show_side || c.watch !== prev.watch) app.refreshChrome();
}

/** Merges settings, applies them and stores them in the backend. */
app.patchConfig = async (patch) => {
  const prev = app.cfg;
  app.cfg = { ...prev, ...patch };
  applyLocal(prev);
  const classes = await api.setConfig(app.cfg);
  if (app.cfg.classes !== prev.classes) await classesChanged(classes);
  if (app.cfg.ui_scale !== prev.ui_scale || app.cfg.min_tile !== prev.min_tile || app.cfg.map_text !== prev.map_text) {
    app.map.syncSize();
    app.map.refresh();
  }
};

async function classesChanged(classes) {
  app.classes = classes;
  app.classVer++;
  app.map.setClasses(classes);
  if (app.tab.mode !== 'map') return;
  app.setStatus(await api.status(app.tab.id));
  app.map.refresh();
}

// ----- tabs -----

const showMode = () => {
  const map = app.tab.mode === 'map';
  $('view').hidden = !map;
  $('start').hidden = map;
  if (!map) app.parts.start.show(app.tab);
};

/** Keeps the field text of the leaving tab. */
function stash(tab) {
  if (!tab) return;
  if (tab.mode === 'map') tab.filterText = app.parts.filter.text();
  else tab.startText = app.parts.start.text();
}

/** Makes a tab the active one. */
app.activate = async (id) => {
  const tab = tabOf(id);
  if (!tab || tab === app.tab) return;
  stash(app.tab);
  app.tab = tab;
  app.hovered = null;
  closeMenu();
  app.parts.tooltip.hide();
  app.parts.tabbar.render();
  showMode();
  if (tab.mode !== 'map') return;
  app.parts.filter.set(tab.filterText, tab.filterError);
  app.map.switchTo(tab.id);
  app.setStatus(await api.status(tab.id));
  app.map.refresh();
};

/** Opens a tab, scanning `path` in it when given. */
app.newTab = async (path = '', activate = true) => {
  const tab = newTab();
  app.tabs.push(tab);
  const active = activate || !app.tab;
  if (active) {
    stash(app.tab);
    app.tab = tab;
    app.hovered = null;
  }
  app.parts.tabbar.render();
  if (active) showMode();
  if (path) await app.scan(tab, path);
  return tab;
};

/** Opens one tab per path, activating the last one. */
app.openPaths = async (paths) => {
  for (const [i, p] of paths.entries()) await app.newTab(p, i === paths.length - 1);
};

app.closeTab = async (id) => {
  const i = app.tabs.findIndex((t) => t.id === id);
  if (i < 0) return;
  const [tab] = app.tabs.splice(i, 1);
  api.closeTab(id);
  app.map.forget(id);
  if (!app.tabs.length) return app.newTab();
  if (tab !== app.tab) return app.parts.tabbar.render();
  const next = app.tabs[Math.min(i, app.tabs.length - 1)];
  app.tab = null;
  await app.activate(next.id);
};

/** Switches to the next (1) or previous (-1) tab. */
app.cycle = (dir) => {
  const i = app.tabs.indexOf(app.tab);
  app.activate(app.tabs[(i + dir + app.tabs.length) % app.tabs.length].id);
};

/** Scans a path in a tab, turning a start tab into a map tab. */
app.scan = async (tab, path) => {
  try {
    const st = await api.scan(tab.id, path);
    tab.mode = 'map';
    tab.path = st.path;
    tab.filterText = '';
    tab.filterError = '';
    app.parts.tabbar.update(tab);
    if (tab !== app.tab) return app.setStatus(st);
    app.parts.filter.set('', '');
    showMode();
    app.map.switchTo(tab.id);
    app.setStatus(st);
    app.map.refresh();
  } catch (e) {
    tab.startError = String(e);
    if (tab === app.tab) app.parts.start.error(tab.startError);
  }
};

/** Shows the hovered or selected path in the status bar. */
app.showPath = async () => {
  const tab = app.tab;
  const node = app.hovered ?? tab.status.selection;
  if (tab.mode !== 'map' || node == null) return app.parts.statusbar.setPath('');
  const d = await app.details(node).catch(() => null);
  const current = app.hovered ?? tab.status.selection;
  if (d && tab === app.tab && current === node) app.parts.statusbar.setPath(d.path);
};
