// Tab model: one tab per scan, or a start screen waiting for a path.

let nextId = 1;

/** Creates an empty start tab. */
export function newTab() {
  return {
    id: nextId++,
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
