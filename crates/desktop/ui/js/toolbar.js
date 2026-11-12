// Toolbar row: navigation, breadcrumb and view toggles.
import { $, el } from './dom.js';
import { setIcon } from './icons.js';

const TOGGLES = { 'b-classes': 'class_colors', 'b-free': 'show_free', 'b-unknown': 'show_unknown' };
const KIND = { 'b-classes': 'classes', 'b-free': 'free', 'b-unknown': 'unknown' };

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
  for (const id of Object.keys(TOGGLES)) $(id).onclick = () => app.toggle(KIND[id]);
  $('b-watch').onclick = () => app.toggle('watch');
  return (st) => update(app, st);
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
  $('b-watch').classList.toggle('on', app.cfg.watch);
  $('b-side').classList.toggle('on', app.cfg.show_side);
  $('viewsize').textContent = st.view_size ?? '';
  renderCrumbs(app, st.crumbs ?? []);
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
