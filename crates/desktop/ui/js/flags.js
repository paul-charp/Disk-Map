// Test switches from the command line (--detail, --side, --compare, --zoom, --select, --hover, --tooltip), applied to the last tab.
import { api } from './api.js';

const SETTLE_MS = 600;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** `latest` is the newest snapshot not taken by this scan. */
async function compareTo(app, tab, which) {
  const list = await api.snapshots(tab.id).catch(() => []);
  const path = which === 'latest' ? list.find((s) => !s.this_scan)?.path : which;
  if (path) await app.compare(path);
}

/** Applies the switches once the scan of `tab` is done. */
export async function applyScanFlags(app, tab, flags) {
  if (flags.side && !app.cfg.show_side) await app.toggle('side');
  if (flags.detail) await app.act(api.setDetail(tab.id, flags.detail));
  if (flags.compare) await compareTo(app, tab, flags.compare);
  const node = async (path) => (path ? api.findNode(tab.id, path) : null);
  const zoom = await node(flags.zoom);
  if (zoom != null) await app.zoom(zoom);
  const select = await node(flags.select);
  if (select != null) await app.select(select);
  const hover = await node(flags.tooltip ?? flags.hover);
  if (hover != null) {
    await wait(SETTLE_MS);
    app.map.hoverTo(hover, !!flags.tooltip);
  }
}
