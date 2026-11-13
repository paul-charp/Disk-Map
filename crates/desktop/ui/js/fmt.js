// Sizes and times for the start screen, matching the backend's `fmt` module.
const UNITS = ['B', 'KB', 'MB', 'GB', 'TB', 'PB'];
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
export const DAY = 86400;

/** Binary-unit size with three significant digits, e.g. `38.2 GB`. */
export function size(bytes) {
  if (bytes < 1024) return `${bytes} B`;
  let v = bytes;
  let unit = 0;
  while (v >= 1024 && unit < UNITS.length - 1) {
    v /= 1024;
    unit++;
  }
  const text = v < 9.995 ? v.toFixed(2) : v < 99.95 ? v.toFixed(1) : v.toFixed(0);
  return `${text.includes('.') ? text.replace(/\.?0+$/, '') : text} ${UNITS[unit]}`;
}

/** Size change with its sign: `+1.2 GB`, `-512 MB`, `0 B`. */
export const signed = (delta) => (delta > 0 ? '+' : delta < 0 ? '-' : '') + size(Math.abs(delta));

/** `up`, `down` or `''` for the colour class of a change. */
export const trend = (delta) => (delta > 0 ? 'up' : delta < 0 ? 'down' : '');

const pad = (n) => String(n).padStart(2, '0');

/** `Nov 3`, with the year when it is not the current one. */
export function day(secs) {
  const d = new Date(secs * 1000);
  const year = d.getFullYear() === new Date().getFullYear() ? '' : ` ${d.getFullYear()}`;
  return `${MONTHS[d.getMonth()]} ${d.getDate()}${year}`;
}

/** Local `HH:MM`. */
export const clock = (d) => `${pad(d.getHours())}:${pad(d.getMinutes())}`;

/** `Nov 3 14:05`. */
export const dayTime = (secs) => `${day(secs)} ${clock(new Date(secs * 1000))}`;

/** `Tue 09:00`. */
export const weekdayTime = (d) => `${WEEKDAYS[d.getDay()]} ${clock(d)}`;

/** Short age: `now`, `12m`, `5h`, `3d`, then the date. */
export function ago(secs, now = Date.now() / 1000) {
  const s = Math.max(0, now - secs);
  if (s < 60) return 'now';
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  if (s < DAY) return `${Math.floor(s / 3600)}h`;
  if (s < 7 * DAY) return `${Math.floor(s / DAY)}d`;
  return day(secs);
}

/** Label of a time axis tick for a step unit (`h`, `d`, `m`, `y`). */
export function tickLabel(secs, unit) {
  const d = new Date(secs * 1000);
  if (unit === 'h') return d.getHours() === 0 ? `${MONTHS[d.getMonth()]} ${d.getDate()}` : clock(d);
  if (unit === 'd') return `${MONTHS[d.getMonth()]} ${d.getDate()}`;
  if (unit === 'm' && d.getMonth() !== 0) return MONTHS[d.getMonth()];
  return String(d.getFullYear());
}
