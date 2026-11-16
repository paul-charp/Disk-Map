// Updates: the startup check, the status bar chip and the About rows.
import { api } from './api.js';
import { button, iconButton, row, segmented, toggle } from './controls.js';
import { $, el } from './dom.js';

const CHANNELS = [['stable', 'Stable'], ['beta', 'Beta'], ['dev', 'Dev']];

/** Wires the chip and runs the startup check; returns `{rows()}` for the About section and `info()`, the last check. */
export function initUpdate(app) {
  const chip = $('st-update');
  let info = null;
  let busy = '';
  let failed = false;
  const views = new Set();

  const render = () => {
    const available = info?.state === 'available';
    chip.hidden = !available;
    chip.textContent = available ? `Update ${info.version}` : '';
    for (const view of views) (view.isConnected ? view.render() : views.delete(view));
    app.parts.start?.status();
  };
  const run = async (word, work) => {
    busy = word;
    failed = false;
    render();
    await work().catch(() => (failed = true));
    busy = '';
    render();
  };
  const check = () => run('Checking', async () => (info = await api.updateCheck()));
  const install = () => run('Installing', () => api.updateInstall());

  chip.title = 'Show in About';
  chip.onclick = () => app.parts.settings.open('About');
  if (app.cfg.check_updates) check();

  const word = () => {
    if (busy) return busy;
    if (failed || info?.state === 'failed') return 'Failed';
    if (info?.state === 'off') return 'Off';
    if (info?.state === 'available') return `${info.version} ${info.downgrade ? 'downgrade' : 'available'}`;
    if (info?.state === 'latest') return 'Latest';
    return '—';
  };

  const statusRow = () => {
    const box = el('span', 'aupd');
    const text = el('span', 'mono');
    const action = el('span', 'alinks');
    const install_ = button('Install', install);
    install_.title = 'Download, install and restart';
    const release = button('Release', () => api.openLink(`release:${info.version}`));
    release.title = 'Open the release page';
    action.append(install_, release);
    const again = iconButton('rescan', 'Check now', check);
    box.append(text, action, again);
    box.render = () => {
      text.textContent = word();
      const available = info?.state === 'available' && !busy;
      install_.hidden = !(available && info.installable);
      release.hidden = !(available && !info.installable);
      again.disabled = !!busy || info?.state === 'off';
    };
    box.render();
    views.add(box);
    return row('Updates', box);
  };

  return {
    info: () => info,
    rows: () => [
      statusRow(),
      row('Channel', segmented(CHANNELS, app.cfg.update_channel, async (v) => {
        await app.patchConfig({ update_channel: v });
        check();
      })),
      row('Auto check', toggle(app.cfg.check_updates, (v) => app.patchConfig({ check_updates: v }))),
    ],
  };
}
