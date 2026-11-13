// Schedule settings: a periodic headless snapshot in Task Scheduler. Changes apply with Apply,
// since an elevated task asks for administrator rights each time it is written.
import { api } from './api.js';
import { el } from './dom.js';
import { button, row, segmented, toggle } from './controls.js';

const EVERY = [['daily', 'Daily'], ['weekly', 'Weekly']];
const DAYS = [['mon', 'Mon'], ['tue', 'Tue'], ['wed', 'Wed'], ['thu', 'Thu'], ['fri', 'Fri'], ['sat', 'Sat'], ['sun', 'Sun']];

const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const sameRoot = (a, b) => a.toLowerCase() === b.toLowerCase();

/** Rows of the Schedule section. */
export function scheduleSection(app) {
  const box = el('div', 'sched');
  box.append(el('div', 'srow muted', '…'));
  api.scheduleGet()
    .then((info) => render(app, box, info))
    .catch((e) => box.replaceChildren(row('Task', el('span', 'muted', String(e)))));
  return [box];
}

function render(app, box, info) {
  let saved = info.config;
  let draft = structuredClone(saved);
  const state = el('span', 'sstate mono');
  const apply = button('Apply', () => save());
  apply.classList.add('primary');

  const refresh = (exists = info.exists) => {
    const changed = !same(draft, saved);
    apply.disabled = !changed;
    state.textContent = changed ? 'Changed' : exists ? 'Active' : saved.enabled ? 'Missing' : 'Off';
    state.title = '';
    day.hidden = draft.every !== 'weekly';
  };
  const set = (key) => (value) => {
    draft[key] = value;
    refresh();
  };

  const save = async () => {
    apply.disabled = true;
    state.textContent = 'Saving';
    try {
      info = await api.scheduleSet(draft);
      saved = info.config;
      draft = structuredClone(saved);
      app.cfg = { ...app.cfg, schedule: saved };
      refresh(info.exists);
    } catch (e) {
      refresh();
      state.textContent = 'Error';
      state.title = String(e);
    }
  };

  const time = el('input', 'stime mono');
  time.type = 'time';
  time.value = draft.time;
  time.onchange = () => set('time')(time.value);

  const elevated = toggle(draft.elevated, set('elevated'));
  elevated.title = info.admin ? 'MFT backend' : 'MFT backend, asks for admin rights';

  const day = row('Day', segmented(DAYS, draft.day, set('day')));
  const status = el('span', 'sstatus');
  status.append(state, apply);

  box.replaceChildren(
    row('Snapshots', toggle(draft.enabled, set('enabled'))),
    row('Repeat', segmented(EVERY, draft.every, set('every'))),
    day,
    row('Time', time),
    row('Folders', roots(info.choices, draft, refresh)),
    row('Elevated', elevated),
    row('Task', status),
  );
  refresh();
}

/** One checkbox per drive or pinned folder. */
function roots(choices, draft, refresh) {
  const list = el('div', 'sroots');
  for (const path of choices) {
    const label = el('label', 'sroot mono');
    const box = el('input');
    box.type = 'checkbox';
    box.checked = draft.roots.some((r) => sameRoot(r, path));
    box.onchange = () => {
      draft.roots = box.checked
        ? [...draft.roots, path]
        : draft.roots.filter((r) => !sameRoot(r, path));
      refresh();
    };
    label.append(box, el('span', '', path));
    list.append(label);
  }
  if (!choices.length) list.append(el('span', 'muted', 'None'));
  return list;
}
