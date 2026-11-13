// Toolbar row: navigation, breadcrumb and view toggles.
import { api } from './api.js';
import { FILE_COLORS } from './app.js';
import { $, el } from './dom.js';
import { setIcon } from './icons.js';
import { openMenu } from './menu.js';

const TOGGLES = { 'b-free': 'show_free', 'b-unknown': 'show_unknown' };
const KIND = { 'b-free': 'free', 'b-unknown': 'unknown' };

/** Menu of the file colour modes under the colours button. */
function colorsMenu(app) {
  const r = $('b-colors').getBoundingClientRect();
  const items = FILE_COLORS.map(([mode, label]) => ({ label, on: app.status.file_colors === mode, run: () => app.setFileColors(mode) }));
  openMenu(items, r.left, r.bottom + 2);
}

/** Wires the toolbar; returns `update(status)`. */
export function initToolbar(app) {
  $('b-viewscan').onclick = () => app.rescanView();
  $('b-back').onclick = () => app.nav('back');
  $('b-fwd').onclick = () => app.nav('forward');
  $('b-up').onclick = () => app.nav('up');
  $('b-home').onclick = () => app.nav('home');
  $('b-scan').onclick = () => app.toggleScan();
  $('b-less').onclick = () => app.changeDetail(-1);
  $('b-more').onclick = () => app.changeDetail(1);
  $('b-side').onclick = () => app.toggle('side');
  $('b-settings').onclick = () => app.parts.settings.open();
  $('b-about').onclick = () => app.parts.settings.open('About');
  $('b-export').onclick = (e) => exportMenu(app, e.currentTarget);
  for (const id of Object.keys(TOGGLES)) $(id).onclick = () => app.toggle(KIND[id]);
  $('b-colors').onclick = () => colorsMenu(app);
  $('b-watch').onclick = () => app.toggle('watch');
  $('b-compare').onclick = (e) => compareMenu(app, e.currentTarget);
  return (st) => update(app, st);
}

/** Snapshots of the root, Save now and Off. */
async function compareMenu(app, button) {
  const list = await api.snapshots(app.tab.id).catch(() => []);
  const rows = list.filter((s) => !s.this_scan);
  const items = rows.length
    ? rows.map((s) => ({ label: `${s.date} · ${s.size}${s.kept ? ' · Kept' : ''}`, on: s.active, run: () => app.compare(s.path) }))
    : [{ title: 'No snapshots' }];
  items.push({ sep: true }, { label: 'Save now', run: () => app.saveSnapshot() });
  if (app.status.compare) items.push({ label: 'Off', run: () => app.compare(null) });
  const r = button.getBoundingClientRect();
  openMenu(items, r.left, r.bottom + 2);
}

function exportMenu(app, button) {
  const items = [];
  const add = (label, scope) =>
    items.push(
      { label: `${label} CSV`, run: () => app.exportTo(scope, 'csv') },
      { label: `${label} JSON`, run: () => app.exportTo(scope, 'json') },
    );
  add('View', 'view');
  if (app.status.selection != null) add('Selection', 'selection');
  items.push({ sep: true });
  add('Top files', 'files');
  add('Top folders', 'folders');
  const r = button.getBoundingClientRect();
  openMenu(items, r.left, r.bottom + 2);
}

function update(app, st) {
  $('b-back').disabled = !st.can_back;
  $('b-fwd').disabled = !st.can_fwd;
  $('b-up').disabled = !st.can_up;
  $('b-home').disabled = !st.can_up;
  $('b-viewscan').disabled = !st.can_rescan_view;
  $('b-less').disabled = st.detail <= 1;
  $('b-more').disabled = st.detail >= 12;
  $('b-scan').dataset.state = st.running ? 'stop' : 'scan';
  setIcon($('b-scan'), st.running ? 'stop' : 'rescan');
  $('b-scan').title = st.running ? 'Stop (F5)' : 'Rescan (F5)';
  for (const [id, key] of Object.entries(TOGGLES)) $(id).classList.toggle('on', !!st[key]);
  updateColors(st.file_colors ?? 'accent');
  $('b-watch').classList.toggle('on', app.cfg.watch);
  $('b-compare').classList.toggle('on', !!st.compare);
  $('b-compare').disabled = !st.active || st.running;
  $('b-side').classList.toggle('on', app.cfg.show_side);
  $('viewsize').textContent = st.view_size ?? '';
  renderCrumbs(app, st.crumbs ?? []);
}

function updateColors(mode) {
  const b = $('b-colors');
  if (b.dataset.mode === mode) return;
  b.dataset.mode = mode;
  b.classList.toggle('on', mode !== 'accent');
  setIcon(b, mode === 'age' ? 'clock' : 'palette');
  const label = FILE_COLORS.find(([m]) => m === mode)?.[1] ?? '';
  b.title = `Colours: ${label} (Ctrl+T)`;
}

function renderCrumbs(app, crumbs) {
  const out = [];
  crumbs.forEach((c, i) => {
    if (i) out.push(el('span', 'crumb-sep', '›'));
    const b = el('button', 'crumb', c.name);
    b.onclick = () => i < crumbs.length - 1 && app.zoom(c.id);
    out.push(b);
  });
  $('crumbs').replaceChildren(...out);
}
