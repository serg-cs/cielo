/**
 * Hash routes:
 *
 * - `#/` saved places and search
 * - `#/m/28079` forecast of a municipality
 * - `#/m/28079/d/2026-11-01` details of one day
 * - `#/m/28079/avisos` warnings of a municipality
 * - `#/p/0301101` beach forecast
 * - `#/p/0301101/avisos` warnings at a beach
 */

/**
 * @typedef {null | {kind: "day", date: string} | {kind: "warnings"}} Sheet
 * @typedef {{view: "locations"} | {view: "forecast", id: string, sheet: Sheet} | {view: "beach", id: string, sheet: Sheet}} Route
 */

/**
 * @param {string} hash
 * @returns {Route}
 */
export function parseRoute(hash) {
  const parts = hash.replace(/^#\/?/, "").split("/").filter(Boolean);
  const id = parts[1] ?? "";
  const view =
    parts[0] === "m" && /^\d{5}$/.test(id) ? "forecast" : parts[0] === "p" && /^\d{7}$/.test(id) ? "beach" : null;
  if (!view) {
    return { view: "locations" };
  }
  if (view === "forecast" && parts[2] === "d" && /^\d{4}-\d{2}-\d{2}$/.test(parts[3] ?? "")) {
    return { view, id, sheet: { kind: "day", date: parts[3] } };
  }
  if (parts[2] === "avisos") {
    return { view, id, sheet: { kind: "warnings" } };
  }
  return { view, id, sheet: null };
}

/**
 * @param {Route} route
 * @returns {string}
 */
export function formatRoute(route) {
  if (route.view === "locations") return "#/";
  const base = `${route.view === "beach" ? "#/p" : "#/m"}/${route.id}`;
  if (route.sheet?.kind === "day") return `${base}/d/${route.sheet.date}`;
  if (route.sheet?.kind === "warnings") return `${base}/avisos`;
  return base;
}

/**
 * Link to a saved place: a municipality or a beach.
 *
 * @param {string} id
 */
export function placeHref(id) {
  return id.length === 7 ? `#/p/${id}` : `#/m/${id}`;
}
