// Start screen of a tab: path field, then the dashboard: drives, trend, movers, pinned folders, recent scans and status slots.
import { api } from './api.js';
import { Trend, change, sparkline } from './chart.js';
import { button, iconButton } from './controls.js';
import { $, el } from './dom.js';
import { compareTo } from './flags.js';
import { ago, dayTime, signed, size, trend, weekdayTime } from './fmt.js';
import { setIcon } from './icons.js';

const WEEKDAYS = ['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'];
const MOVERS_SHOWN = 5;

/** Joins a scan root and a path below it. */
const join = (root, rel) => (rel ? (root.endsWith('\\') ? root + rel : `${root}\\${rel}`) : root);

/** A signed size coloured by direction, or a muted placeholder. */
function delta(value, cls = 'd') {
  if (value == null) return el('span', `${cls} muted`, '');
  return el('span', `${cls} ${trend(value)}`, signed(value));
}

/** Next local start of the snapshot task, or null when it is off. */
export function nextRun(s, now = new Date()) {
  if (!s.enabled) return null;
  const [h, m] = s.time.split(':').map(Number);
  const want = s.every === 'weekly' ? WEEKDAYS.indexOf(s.day) : -1;
  const d = new Date(now);
  d.setHours(h, m, 0, 0);
  for (let i = 0; i < 8; i++, d.setDate(d.getDate() + 1)) {
    if (d > now && (want < 0 || d.getDay() === want)) return d;
  }
  return null;
}

function tag(text, title = '', cls = '') {
  const t = el('span', `tag ${cls}`.trim(), text);
  t.title = title;
  return t;
}

function driveCard(d, ctx) {
  const card = el('div', 'dcard');
  card.tabIndex = 0;
  card.classList.toggle('on', d.root === ctx.chosen());
  card.title = `Scan ${d.root}`;
  const used = Math.max(0, d.total - d.free);
  const head = el('div', 'dh');
  head.append(el('span', 'l', d.root.replace(/\\$/, '')), el('span', 't', d.label || d.kind));
  if (d.kind !== 'Fixed') head.append(tag(d.kind));
  if (d.file_system) head.append(tag(d.file_system));
  if (d.mft) head.append(tag('MFT', ctx.admin ? 'MFT scan' : 'MFT scan when elevated', ctx.admin ? 'on' : ''));
  if (d.points.length) {
    const b = iconButton('chart', 'Trend', (e) => {
      e.stopPropagation();
      ctx.choose(d.root);
    });
    b.classList.add('dtrend');
    head.append(b);
  }
  const bar = el('div', 'u');
  const fill = el('i');
  fill.style.width = `${((used / Math.max(d.total, 1)) * 100).toFixed(1)}%`;
  bar.append(fill);
  const usage = el('div', 'dm');
  usage.append(el('span', 's', `${size(used)} of ${size(d.total)}`), el('span', 'muted', `${size(d.free)} free`));
  card.append(head, bar, usage);
  if (d.points.length) card.append(changes(d.points));
  card.onclick = () => ctx.scan(d.root);
  card.onkeydown = (e) => e.key === 'Enter' && ctx.scan(d.root);
  return card;
}

/** The 7 and 30 day changes and the sparkline. */
function changes(points) {
  const row = el('div', 'dd');
  for (const days of [7, 30]) {
    const v = change(points, days);
    const cell = el('span', 'dc');
    cell.append(el('span', 'muted', `${days}d`), v == null ? el('span', 'muted', '—') : delta(v, 'v'));
    row.append(cell);
  }
  if (points.length > 1) row.append(sparkline(points));
  return row;
}

function pinButton(r, refresh) {
  const b = el('button', r.pinned ? 'ib pin on' : 'ib pin');
  b.title = r.pinned ? 'Unpin' : 'Pin';
  setIcon(b, 'pin');
  b.onclick = async (e) => {
    e.stopPropagation();
    await api.setPin(r.path, !r.pinned);
    refresh();
  };
  return b;
}

function pathRow(r, ctx) {
  const row = el('div', 'item recent');
  const when = el('span', 'a', r.time ? ago(r.time) : '');
  if (r.time) when.title = dayTime(r.time);
  const d = delta(r.delta);
  d.title = r.delta == null ? '' : 'Since previous scan';
  row.append(el('span', 't', r.path), when, d, el('span', 's', r.size ? size(r.size) : ''), pinButton(r, ctx.refresh));
  row.title = r.path;
  row.onclick = () => ctx.scan(r.path);
  return row;
}

function compareButton(ctx, root, rel = '') {
  const b = iconButton('compare', 'Compare', (e) => {
    e.stopPropagation();
    ctx.compare(root, rel);
  });
  b.classList.add('cmp');
  return b;
}

/** A mover below a report line. */
function moverRow(r, m, ctx) {
  const row = el('div', 'item sub');
  row.title = `Scan ${join(r.root, m.path)}`;
  row.append(el('span', 't', m.path), delta(m.delta));
  if (r.snapshot) row.append(compareButton(ctx, r.root, m.path));
  row.onclick = () => ctx.open(join(r.root, m.path));
  return row;
}

/** A report line: root, time and total change; the chevron shows its movers. */
function reportRow(r, ctx) {
  const key = `${r.root}|${r.time}`;
  const box = el('div', 'mv');
  const row = el('div', 'item');
  row.title = `Scan ${r.root}`;
  const movers = [...r.growers.slice(0, MOVERS_SHOWN), ...r.shrinkers.slice(0, MOVERS_SHOWN)];
  const list = el('div', 'mlist');
  list.append(...movers.map((m) => moverRow(r, m, ctx)));
  const open = () => ctx.expanded.has(key);
  const chev = el('button', 'ib chev');
  chev.title = 'Movers';
  setIcon(chev, 'chevron');
  chev.disabled = !movers.length;
  const sync = () => {
    list.hidden = !open();
    chev.classList.toggle('open', open());
  };
  chev.onclick = (e) => {
    e.stopPropagation();
    if (open()) ctx.expanded.delete(key);
    else ctx.expanded.add(key);
    sync();
  };
  const when = el('span', 'a', ago(r.time));
  when.title = dayTime(r.time);
  const total = r.old == null ? el('span', 'd muted', 'First') : delta(r.new - r.old);
  row.append(chev, el('span', 't', r.root), when, total);
  if (r.snapshot) row.append(compareButton(ctx, r.root));
  row.onclick = () => ctx.open(r.root);
  sync();
  box.append(row, list);
  return box;
}

/** "No history" style placeholder with a Schedule button when the task is off. */
function empty(text, schedule, ctx) {
  const box = el('div', 'empty');
  box.append(el('span', 'muted', text));
  if (!schedule.enabled) box.append(button('Schedule', ctx.schedule));
  return box;
}

function slot(icon, text, title, run, cls = '') {
  const b = el('button', `slot ${cls}`.trim());
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  setIcon(svg, icon);
  b.append(svg, el('span', '', text));
  b.title = title;
  if (run) b.onclick = run;
  else b.disabled = true;
  return b;
}

/** Wires the start screen; returns `{show(tab), text(), error(text), status()}`. */
export function initStart(app) {
  const input = $('start-path');
  let data = null;
  let chosen = null;
  let seq = 0;
  let taskMissing = false;
  const chart = new Trend($('trend'), (p, prev) => {
    $('trend-read').textContent = p ? `${dayTime(p.time)} · ${size(p.allocated)}${prev ? ` · ${signed(p.allocated - prev.allocated)}` : ''}` : '';
  });

  /** Scans the first of the `;`-separated paths in this tab and opens the others in new tabs. */
  const scan = async (text) => {
    const [first, ...rest] = text.split(';').map((p) => p.trim()).filter(Boolean);
    if (!first) return;
    const tab = app.tab;
    for (const p of rest) await app.newTab(p, false);
    await app.scan(tab, first);
  };

  /** Opens a tab scanning `root`, compares it with the latest snapshot when the scan ends, then zooms to `rel`. */
  const compare = async (root, rel) => {
    const tab = await app.newTab();
    app.afterScan(tab, async () => {
      if (!(await compareTo(app, tab, 'latest'))) return app.parts.statusbar.flash('No snapshot');
      const node = rel ? await api.findNode(tab.id, rel).catch(() => null) : null;
      if (node != null && app.tab === tab) await app.zoom(node);
    });
    await app.scan(tab, root);
  };

  const ctx = {
    scan,
    compare,
    admin: false,
    expanded: new Set(),
    open: (path) => app.newTab(path),
    chosen: () => chosen,
    choose: (root) => {
      chosen = root;
      render();
    },
    schedule: () => app.parts.settings.open('Schedule'),
    refresh: () => refresh(),
  };

  const renderTrend = () => {
    const withHistory = data.drives.filter((d) => d.points.length);
    if (!withHistory.some((d) => d.root === chosen)) chosen = withHistory[0]?.root ?? null;
    const drive = withHistory.find((d) => d.root === chosen);
    $('trend').hidden = !drive;
    $('trend-empty').hidden = !!drive;
    $('trend-empty').replaceChildren(...(drive ? [] : empty('No history', data.schedule, ctx).childNodes));
    const pick = $('trend-pick');
    pick.hidden = withHistory.length < 2;
    pick.replaceChildren(
      ...withHistory.map((d) => {
        const b = el('button', d.root === chosen ? 'on' : '', d.root.replace(/\\$/, ''));
        b.title = d.label || d.root;
        b.onclick = () => ctx.choose(d.root);
        return b;
      }),
    );
    chart.set(drive ? drive.points : []);
  };

  const renderSlots = () => {
    const slots = [];
    const update = app.parts.update?.info();
    if (update?.state === 'available') {
      slots.push(slot('down', `Update ${update.version}`, 'Show in About', () => app.parts.settings.open('About'), 'accent'));
    }
    if (data) {
      const next = nextRun(data.schedule);
      if (taskMissing) slots.push(slot('warning', 'Task missing', 'Apply the schedule again', ctx.schedule, 'warn'));
      else if (next) slots.push(slot('clock', weekdayTime(next), `Next snapshot ${dayTime(next.getTime() / 1000)}`, ctx.schedule));
      else slots.push(slot('clock', 'Schedule off', 'Schedule', ctx.schedule, 'dim'));
      if (data.admin) slots.push(slot('shield', 'Admin', 'Elevated: MFT scans', null, 'dim'));
      else if (data.drives.some((d) => d.mft)) slots.push(slot('shield', 'Elevate', 'Restart as administrator for MFT scans', () => api.elevate(app.tab.id)));
    }
    $('start-slots').replaceChildren(...slots);
  };

  const render = () => {
    if (!data) return;
    ctx.admin = data.admin;
    $('start-drives').replaceChildren(...data.drives.map((d) => driveCard(d, ctx)));
    renderTrend();
    $('start-movers').replaceChildren(...(data.reports.length ? data.reports.map((r) => reportRow(r, ctx)) : [empty('No reports', data.schedule, ctx)]));
    $('start-pinned').hidden = data.pins.length === 0;
    $('start-pins').replaceChildren(...data.pins.map((r) => pathRow(r, ctx)));
    $('start-recent').replaceChildren(...(data.recent.length ? data.recent.map((r) => pathRow(r, ctx)) : [el('div', 'empty muted', 'None')]));
    renderSlots();
  };

  /** Checks in the background that an enabled task is really registered. */
  const checkTask = async (schedule) => {
    if (!schedule.enabled) return (taskMissing = false);
    const info = await api.scheduleGet().catch(() => null);
    taskMissing = !!info && info.config.enabled && !info.exists;
    renderSlots();
  };

  const refresh = async () => {
    const mine = ++seq;
    const next = await api.dashboard().catch((e) => {
      $('start-err').textContent = String(e);
      return null;
    });
    if (!next || mine !== seq) return;
    data = next;
    app.driveLabels = new Map(data.drives.map((d) => [d.root, d.label]));
    app.parts.tabbar.render();
    render();
    checkTask(data.schedule);
  };

  $('start-go').onclick = () => scan(input.value);
  $('start-browse').onclick = async () => {
    const picked = await api.browse();
    if (picked) scan(picked);
  };
  input.addEventListener('keydown', (e) => e.key === 'Enter' && scan(input.value));
  return {
    text: () => input.value,
    error: (text) => {
      $('start-err').textContent = text;
    },
    status: () => !$('start').hidden && renderSlots(),
    show: async (tab) => {
      input.value = tab.startText;
      $('start-err').textContent = tab.startError;
      input.focus();
      await refresh();
    },
  };
}
