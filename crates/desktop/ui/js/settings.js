// Settings dialog: a modal window with a list of sections; every change applies at once.
import { api } from './api.js';
import { $, el } from './dom.js';
import { classesSection } from './classtable.js';
import { button, iconButton, row, segmented, stepper, swatches, toggle } from './controls.js';
import { setIcon } from './icons.js';
import { SHORTCUTS } from './keys.js';
import { THEMES } from './themes.js';

const SCALES = [80, 90, 100, 110, 125, 150, 175, 200];
const THREADS = [0, 1, 2, 3, 4, 6, 8, 12, 16, 24, 32, 48, 64];
const TEXT = [['small', 'Small'], ['medium', 'Medium'], ['large', 'Large']];
const BACKENDS = [['auto', 'Auto'], ['mft', 'MFT'], ['walk', 'Walk']];
const FOCUSABLE = 'button:not(:disabled), input';

/** Wires the dialog; returns `{open(section), close(), toggle(), isOpen()}`. */
export function initSettings(app) {
  const root = $('settings');
  const set = (key) => (value) => app.patchConfig({ [key]: value });
  const num = (key, values, format) => stepper(values, app.cfg[key], format, set(key));
  const flag = (key) => toggle(app.cfg[key], set(key));

  const sections = {
    Appearance: () => [
      row('Theme', swatches(THEMES, app.cfg.theme, set('theme'))),
      row('UI scale', num('ui_scale', SCALES, (v) => `${v}%`)),
      row('Map text', segmented(TEXT, app.cfg.map_text, set('map_text'))),
      row('Animation', flag('animation')),
    ],
    Map: () => [
      row('Detail', num('detail', [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], String)),
      row('Min tile', num('min_tile', [2, 3, 4, 5, 6, 8, 10, 12, 16], (v) => `${v} px`)),
      row('Class colours', flag('class_colors')),
      row('Free space', flag('show_free')),
      row('Unknown space', flag('show_unknown')),
    ],
    Scan: () => [
      row('Backend', segmented(BACKENDS, app.cfg.backend, set('backend'))),
      row('Threads', num('threads', THREADS, (v) => (v ? String(v) : 'Auto'))),
      row('Watch changes', flag('watch')),
      row('Highlight changes', flag('highlight_changes')),
    ],
    Classes: () => [classesSection(app)],
    Keys: () => [keyList()],
    About: () => aboutSection(),
  };

  const keyList = () => {
    const g = el('div', 'keys');
    for (const [k, a] of SHORTCUTS) g.append(el('span', 'mono', k), el('span', '', a));
    return g;
  };

  let current = 'Appearance';
  let before = null;
  const nav = el('nav', 'snav');
  const body = el('div', 'sbody');

  const show = (name) => {
    current = name;
    for (const b of nav.children) b.setAttribute('aria-selected', b.textContent === name);
    body.replaceChildren(...sections[name]());
    body.dataset.section = name;
  };

  for (const name of Object.keys(sections)) {
    const b = el('button', '', name);
    b.setAttribute('role', 'tab');
    b.onclick = () => show(name);
    nav.append(b);
  }

  const close = el('button', 'ib');
  close.title = 'Close (Esc)';
  setIcon(close, 'close');
  const title = el('div', 'wtitle');
  title.append(el('span', '', 'Settings'), close);
  const win = el('div', 'win');
  win.setAttribute('role', 'dialog');
  win.setAttribute('aria-label', 'Settings');
  const wbody = el('div', 'wbody');
  wbody.append(nav, body);
  win.append(title, wbody);
  root.append(win);

  const api = {
    isOpen: () => !root.hidden,
    open: (section) => {
      const name = Object.keys(sections).find((s) => s.toLowerCase() === String(section).toLowerCase()) ?? current;
      before = document.activeElement;
      show(name);
      root.hidden = false;
      [...nav.children].find((b) => b.textContent === name).focus();
    },
    close: () => {
      root.hidden = true;
      before?.focus?.();
    },
    toggle: () => (api.isOpen() ? api.close() : api.open()),
  };

  close.onclick = api.close;
  root.addEventListener('mousedown', (e) => e.target === root && api.close());
  root.addEventListener('keydown', (e) => onKey(e, api, nav, win));
  return api;
}

let aboutInfo = null;

/** Name, version, license, links, config path and build of the app. */
function aboutSection() {
  const logo = document.querySelector('#start .logo')?.cloneNode(true);
  const name = el('span', 'aname', 'diskmap');
  const version = el('span', 'mono muted');
  const head = el('div', 'ahead');
  head.append(...(logo ? [logo] : []), name, version);
  const license = el('span');
  const links = el('span', 'alinks');
  links.append(button('GitHub', () => api.openLink('repo')), button('Issues', () => api.openLink('issues')));
  const path = el('span', 'apath mono');
  const config = el('span', 'acfg');
  config.append(path, iconButton('folder', 'Reveal', () => api.revealConfig()));
  const build = el('span', 'mono muted');
  aboutInfo ??= api.about();
  aboutInfo.then((a) => {
    name.textContent = a.name;
    version.textContent = a.version;
    license.textContent = `${a.license} License`;
    path.textContent = a.config || '—';
    path.title = a.config;
    build.textContent = `${a.target} · ${a.profile}`;
  }).catch(() => {
    aboutInfo = null;
  });
  return [head, row('License', license), row('Links', links), row('Config', config), row('Build', build)];
}

function onKey(e, api, nav, win) {
  if (e.key === 'Escape') {
    e.preventDefault();
    return api.close();
  }
  if (e.target.parentElement === nav && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
    e.preventDefault();
    const items = [...nav.children];
    const next = items[(items.indexOf(e.target) + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length];
    next.click();
    return next.focus();
  }
  if (e.key !== 'Tab') return;
  const items = [...win.querySelectorAll(FOCUSABLE)];
  const edge = e.shiftKey ? items[0] : items.at(-1);
  if (document.activeElement !== edge) return;
  e.preventDefault();
  (e.shiftKey ? items.at(-1) : items[0]).focus();
}
