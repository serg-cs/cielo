/**
 * Selecting the warnings that apply to a municipality.
 */

import { WARNING_LEVELS } from "./codes.js";

/**
 * @typedef {object} Warning
 * @property {string} id
 * @property {"yellow" | "orange" | "red"} level
 * @property {string} [code] Meteoalerta phenomenon code, such as `PR`.
 * @property {string} name Phenomenon in Spanish, such as `Lluvias`.
 * @property {string} [onset]
 * @property {string} [expires]
 * @property {string} [desc]
 * @property {string} [param]
 * @property {string} [prob]
 * @property {string} [area]
 * @property {string[]} zones
 * @property {string[]} [m]
 */

/**
 * Warnings for a municipality that have not expired, most severe first.
 *
 * @param {any} document `warnings.json`.
 * @param {string} id Municipality INE code.
 * @param {string | undefined} zone Its Meteoalerta zone.
 * @param {number} now
 * @returns {Warning[]}
 */
export function warningsFor(document, id, zone, now) {
  const warnings = Array.isArray(document?.warnings) ? document.warnings : [];
  return warnings
    .filter((warning) => {
      const applies =
        (zone && Array.isArray(warning.zones) && warning.zones.includes(zone)) ||
        (Array.isArray(warning.m) && warning.m.includes(id));
      const expires = warning.expires ? Date.parse(warning.expires) : Number.POSITIVE_INFINITY;
      return applies && expires > now && WARNING_LEVELS[warning.level];
    })
    .sort(
      (left, right) =>
        WARNING_LEVELS[right.level].rank - WARNING_LEVELS[left.level].rank ||
        Date.parse(left.onset ?? "") - Date.parse(right.onset ?? ""),
    );
}

/**
 * Most severe level among warnings, or `null`.
 *
 * @param {Warning[]} warnings
 */
export function highestLevel(warnings) {
  return warnings[0]?.level ?? null;
}
