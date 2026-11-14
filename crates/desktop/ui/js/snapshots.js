// Snapshots settings section: every stored snapshot, grouped by root, with Compare, Keep, Reveal and Delete.
import { api } from './api.js';
import { el } from './dom.js';
import { iconButton, row } from './controls.js';
import { confirmMenu } from './menu.js';

/** Rows of the Snapshots section. */
export function snapshotsSection(app) {
  const box = el('div', 'snaps');
  const load = () =>
    api.snapshotStore()
      .then((store) => render(app, box, store, load))
      .catch((e) => box.replaceChildren(row('Stored', el('span', 'muted', String(e)))));
  box.append(el('div', 'srow muted', '…'));
  load();
  return [box];
}

function render(app, box, store, reload) {
  const total = el('span', 'snum mono', store.count ? `${store.count} · ${store.file}` : 'None');
  const head = el('span', 'acfg');
  head.append(total, iconButton('folder', 'Open folder', () => api.revealSnapshot()));
  const parts = [row('Stored', head)];
  for (const g of store.groups) parts.push(group(app, g, reload));
  box.replaceChildren(...parts);
}

function group(app, g, reload) {
  const sec = el('section', 'sgroup');
  const kept = g.rows.filter((r) => r.kept).length;
  const title = el('div', 'sghead');
  const name = el('span', 'sgroot mono', g.root);
  name.title = g.root;
  const pruned = g.rows.filter((r) => !r.kept);
  const clear = iconButton('trash', 'Delete unkept', (e) =>
    confirm(e, `${pruned.length} unkept`, 'Delete', async () => {
      for (const r of pruned) await api.deleteSnapshot(r.path).catch(() => {});
      reload();
    }),
  );
  clear.disabled = !pruned.length;
  title.append(
    name,
    el('span', 'muted mono', `${g.rows.length}${kept ? ` · ${kept} kept` : ''} · ${g.file}`),
    iconButton('compare', 'Compare latest', () => openCompare(app, g.root, 'latest')),
    clear,
  );
  sec.append(title, ...g.rows.map((r) => snapRow(app, g.root, r, reload)));
  return sec;
}

function snapRow(app, root, r, reload) {
  const line = el('div', 'snap');
  line.append(
    el('span', 'sdate mono', r.date),
    el('span', 'ssize mono', r.size),
    el('span', 'sfile mono muted', r.file),
  );
  line.title = `${r.size} · ${r.files.toLocaleString()} files · ${r.file}`;
  const keep = iconButton('pin', r.kept ? 'Unkeep' : 'Keep', () =>
    api.keepSnapshot(r.path, !r.kept).then(reload, (e) => app.parts.statusbar.flash(String(e))),
  );
  keep.classList.toggle('on', r.kept);
  line.append(
    keep,
    iconButton('compare', 'Compare', () => openCompare(app, root, r.path)),
    iconButton('folder', 'Reveal', () => api.revealSnapshot(r.path)),
    iconButton('trash', 'Delete', (e) =>
      confirm(e, r.date, 'Delete', () => api.deleteSnapshot(r.path).then(reload, (err) => app.parts.statusbar.flash(String(err)))),
    ),
  );
  return line;
}

function confirm(e, title, label, run) {
  const b = e.currentTarget.getBoundingClientRect();
  confirmMenu(title, label, run, b.left, b.bottom + 2);
}

function openCompare(app, root, which) {
  app.parts.settings.close();
  app.compareRoot(root, which);
}
