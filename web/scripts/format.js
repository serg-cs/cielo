/**
 * Spanish formatting of times, dates and values in a municipality's time zone.
 */

const LOCALE = "es-ES";
/** @type {Map<string, Intl.DateTimeFormat>} */
const formatters = new Map();

/**
 * @param {string} timeZone
 * @param {Intl.DateTimeFormatOptions} options
 */
function formatter(timeZone, options) {
  const key = `${timeZone}|${JSON.stringify(options)}`;
  let cached = formatters.get(key);
  if (!cached) {
    cached = new Intl.DateTimeFormat(LOCALE, { timeZone, ...options });
    formatters.set(key, cached);
  }
  return cached;
}

/**
 * Local calendar date as `YYYY-MM-DD`.
 *
 * @param {Date | number} time
 * @param {string} timeZone
 */
export function localDate(time, timeZone) {
  const parts = formatter(timeZone, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(time);
  const value = (/** @type {string} */ type) => parts.find((part) => part.type === type)?.value;
  return `${value("year")}-${value("month")}-${value("day")}`;
}

/**
 * Local hour of the day (0-23).
 *
 * @param {Date | number} time
 * @param {string} timeZone
 */
export function localHour(time, timeZone) {
  const hour = formatter(timeZone, { hour: "numeric", hourCycle: "h23" })
    .formatToParts(time)
    .find((part) => part.type === "hour")?.value;
  return Number(hour ?? 0);
}

/**
 * `14:05`.
 *
 * @param {Date | number} time
 * @param {string} timeZone
 */
export function clockTime(time, timeZone) {
  return formatter(timeZone, {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
  }).format(time);
}

/**
 * `14` for an hour column.
 *
 * @param {Date | number} time
 * @param {string} timeZone
 */
export function hourLabel(time, timeZone) {
  return String(localHour(time, timeZone)).padStart(2, "0");
}

/**
 * Add days to a `YYYY-MM-DD` date.
 *
 * @param {string} date
 * @param {number} days
 */
export function addDays(date, days) {
  const [year, month, day] = date.split("-").map(Number);
  const value = new Date(Date.UTC(year, month - 1, day + days));
  return value.toISOString().slice(0, 10);
}

/**
 * Weekday of a `YYYY-MM-DD` date.
 *
 * @param {string} date
 * @param {"long" | "short"} [style]
 */
export function weekday(date, style = "long") {
  const [year, month, day] = date.split("-").map(Number);
  return formatter("UTC", { weekday: style }).format(Date.UTC(year, month - 1, day));
}

/**
 * `1 de noviembre` or `1 nov`.
 *
 * @param {string} date
 * @param {"long" | "short"} [style]
 */
export function dayMonth(date, style = "long") {
  const [year, month, day] = date.split("-").map(Number);
  return formatter("UTC", { day: "numeric", month: style }).format(Date.UTC(year, month - 1, day));
}

/**
 * `Hoy`, `Mañana` or the weekday.
 *
 * @param {string} date
 * @param {string} today
 * @param {"long" | "short"} [style]
 */
export function dayName(date, today, style = "long") {
  if (date === today) return "Hoy";
  if (date === addDays(today, 1)) return "Mañana";
  return capitalize(weekday(date, style));
}

/** @param {string} text */
export function capitalize(text) {
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : text;
}

/**
 * `14°`, or a dash when missing.
 *
 * @param {number | null | undefined} value
 */
export function degrees(value) {
  return value === null || value === undefined ? "—" : `${signed(value)}°`;
}

/**
 * A number with a typographic minus sign: `−3`.
 *
 * @param {number} value
 */
function signed(value) {
  return value < 0 ? `−${Math.abs(value)}` : String(value);
}

/**
 * `1.250` or `0,4`, Spanish style.
 *
 * @param {number} value
 * @param {number} [digits]
 */
export function number(value, digits = 0) {
  return value.toLocaleString(LOCALE, { maximumFractionDigits: digits });
}

/**
 * When the data was last requested from AEMET, in the device's time:
 * `Actualizado a las 16:17`, or with the date when it was not today.
 *
 * @param {number} time
 * @param {number} now
 * @param {string} [timeZone] Defaults to the device's.
 */
export function builtAt(time, now, timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone) {
  const clock = clockTime(time, timeZone);
  if (localDate(time, timeZone) === localDate(now, timeZone)) return `Actualizado a las ${clock}`;
  return `Actualizado el ${dayMonth(localDate(time, timeZone), "short")}, ${clock}`;
}

/**
 * When something happens, relative to today: `a las 14 h`,
 * `mañana a la 1 h`, `el lunes a las 9 h`.
 *
 * @param {number} time
 * @param {string} timeZone
 * @param {string} today
 */
export function relativeHour(time, timeZone, today) {
  const date = localDate(time, timeZone);
  const value = localHour(time, timeZone);
  const hour = `${value === 1 ? "a la" : "a las"} ${value}\u00a0h`;
  if (date === today) return hour;
  if (date === addDays(today, 1)) return `mañana ${hour}`;
  return `el ${weekday(date)} ${hour}`;
}

/**
 * Precipitation amount: `0,4 mm`, or `unas gotas` for less than 0.1 mm
 * (AEMET's trace, `Ip`).
 *
 * @param {number | string | null | undefined} value
 */
export function millimetres(value) {
  if (value === "Ip" || (typeof value === "number" && value > 0 && value < 0.1)) return "unas gotas";
  if (value === null || value === undefined) return "—";
  return `${number(Number(value), 1)}\u00a0mm`;
}

/**
 * Numeric value of an amount, counting a trace as 0.05 mm.
 *
 * @param {number | string | null | undefined} value
 */
export function amount(value) {
  if (value === "Ip") return 0.05;
  return typeof value === "number" ? value : 0;
}

/**
 * Hours and minutes between two `HH:MM` times: `10 h 31 min`.
 *
 * @param {string} start
 * @param {string} end
 */
export function duration(start, end) {
  const minutes = (time) => Number(time.slice(0, 2)) * 60 + Number(time.slice(3, 5));
  const total = minutes(end) - minutes(start);
  if (!(total > 0)) return "";
  return `${Math.floor(total / 60)}\u00a0h ${String(total % 60).padStart(2, "0")}\u00a0min`;
}

/**
 * `a las 14:30`, or `a la 1:30` for one o'clock, Spanish style.
 *
 * @param {Date | number} time
 * @param {string} timeZone
 */
export function atClock(time, timeZone) {
  const value = localHour(time, timeZone);
  return `${value === 1 ? "a la" : "a las"} ${value}:${clockTime(time, timeZone).slice(3)}`;
}
