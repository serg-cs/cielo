/**
 * Preferences kept in this browser: saved places (municipalities and
 * beaches) and the last one opened.
 */

const SAVED_KEY = "cielo.savedMunicipalityIds";
const LAST_OPENED_KEY = "cielo.lastOpenedMunicipalityId";

/** @param {string} key */
function read(key) {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

/** @param {string} key @param {string | null} value */
function write(key, value) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Storage can be unavailable (private mode); preferences then last for the session.
  }
}

/** @returns {string[]} */
export function savedIds() {
  try {
    const value = JSON.parse(read(SAVED_KEY) ?? "[]");
    return Array.isArray(value) ? value.filter((id) => typeof id === "string" && /^\d{5}(\d{2})?$/.test(id)) : [];
  } catch {
    return [];
  }
}

/** @param {string[]} ids */
export function setSavedIds(ids) {
  write(SAVED_KEY, JSON.stringify([...new Set(ids)]));
}

export const lastOpenedId = () => read(LAST_OPENED_KEY);
/** @param {string | null} id */
export const setLastOpenedId = (id) => write(LAST_OPENED_KEY, id);

const HOURLY_MODE_KEY = "cielo.hourlyMode";

/** @returns {"temp" | "rain" | "wind"} */
export function hourlyMode() {
  const mode = read(HOURLY_MODE_KEY);
  return mode === "rain" || mode === "wind" ? mode : "temp";
}

/** @param {string} mode */
export const setHourlyMode = (mode) => write(HOURLY_MODE_KEY, mode);
