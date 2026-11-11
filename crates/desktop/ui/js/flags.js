// Test switches from the command line (--detail, --side, --zoom, --select, --hover, --tooltip), applied to the last tab.
import { api } from './api.js';

const SETTLE_MS = 600;
const wait = (ms) => new Promise((r) => setTimeout(r, ms));

/** Applies the switches once the scan of `tab` is done. */
export async function applyScanFlags(app, tab, flags) {
  if (flags.side && !app.cfg.show_side) await app.toggle('side');
  if (flags.detail) await app.act(api.setDetail(tab.id, flags.detail));
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
