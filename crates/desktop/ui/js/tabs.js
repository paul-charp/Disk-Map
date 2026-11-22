// Tab model: the Home tab (the start screen, always first) and one tab per scan.

let nextId = 1;

/** Creates a tab waiting for a scan, or the Home tab. */
export function newTab(home = false) {
  return {
    id: nextId++,
    home,
    mode: 'start',
    path: '',
    status: { active: false },
    filterText: '',
    filterError: '',
    startText: '',
    startError: '',
    detailCache: new Map(),
  };
}

/** Short tab name: drive letter and label for a volume root, else the folder name. */
export function titleOf(path, labels) {
  if (!path) return 'New tab';
  const drive = /^([A-Za-z]:)[\\/]?$/.exec(path);
  if (drive) return [drive[1], labels?.get(`${drive[1]}\\`)].filter(Boolean).join(' ');
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? path;
}
