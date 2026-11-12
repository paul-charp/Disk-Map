// Thin wrappers over the Tauri commands. Commands on a scan take the id of its tab.
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

export const api = {
  boot: () => invoke('boot'),
  showWindow: () => invoke('show_window'),
  setConfig: (config) => invoke('set_config', { config }),
  status: (tab) => invoke('get_status', { tab }),
  scan: (tab, path) => invoke('scan_path', { tab, path }),
  rescan: (tab) => invoke('rescan', { tab }),
  rescanView: (tab) => invoke('rescan_view', { tab }),
  stop: (tab) => invoke('stop_scan', { tab }),
  closeTab: (tab) => invoke('close_tab', { tab }),
  layout: (tab, width, height, dpr) => invoke('layout', { tab, width, height, dpr }),
  select: (tab, node) => invoke('select', { tab, node }),
  zoom: (tab, node) => invoke('zoom', { tab, node }),
  back: (tab) => invoke('nav_back', { tab }),
  forward: (tab) => invoke('nav_forward', { tab }),
  up: (tab) => invoke('nav_up', { tab }),
  home: (tab) => invoke('nav_home', { tab }),
  tag: (tab, node, tag) => invoke('tag', { tab, node, tag }),
  clearTags: (tab) => invoke('clear_tags', { tab }),
  setFilter: (tab, text) => invoke('set_filter', { tab, text }),
  setDetail: (tab, detail) => invoke('set_detail', { tab, detail }),
  setToggle: (tab, kind, on) => invoke('set_toggle', { tab, kind, on }),
  details: (tab, node) => invoke('details', { tab, node }),
  stats: (tab) => invoke('stats', { tab }),
  findNode: (tab, path) => invoke('find_node', { tab, path }),
  open: (tab, node) => invoke('open_node', { tab, node }),
  reveal: (tab, node) => invoke('reveal_node', { tab, node }),
  startInfo: () => invoke('start_info'),
  browse: () => invoke('browse'),
  exportTo: (tab, scope, format, depth = null, node = null, path = null) =>
    invoke('export', { tab, scope, format, depth, node, path }),
  elevate: (tab) => invoke('elevate', { tab }),
  perf: (line) => invoke('log_perf', { line }),
};

export const onScan = (fn) => listen('scan', (e) => fn(e.payload));
export const onDrop = (fn) => listen('dropped', (e) => fn(e.payload));
