const collator = new Intl.Collator("es", {
  numeric: true,
  sensitivity: "base",
});
const catalogSchemaVersion = 1;
const supportedTimezones = new Set([
  "Africa/Ceuta",
  "Atlantic/Canary",
  "Europe/Madrid",
]);

export const minimumSearchLength = 2;

/**
 * @typedef {object} Municipality
 * @property {string} id
 * @property {string} name
 * @property {string} province
 * @property {string} timezone
 * @property {string} [searchName]
 */

/**
 * @param {Municipality} left
 * @param {Municipality} right
 * @returns {number}
 */
export function compareMunicipalities(left, right) {
  return (
    collator.compare(left.name, right.name) ||
    collator.compare(left.province, right.province) ||
    collator.compare(left.id, right.id)
  );
}

/**
 * @param {unknown} document
 * @returns {Municipality[]}
 */
export function validateMunicipalities(document) {
  if (
    typeof document !== "object" ||
    document === null ||
    !("schema_version" in document) ||
    document.schema_version !== catalogSchemaVersion ||
    !("municipalities" in document) ||
    !Array.isArray(document.municipalities) ||
    !document.municipalities.every(isMunicipality)
  ) {
    throw new Error("El documento de municipios no es válido");
  }

  return document.municipalities.map((municipality) => ({
    ...municipality,
    searchName: normalizeSearchText(municipality.name),
  }));
}

/**
 * @param {unknown} value
 * @returns {value is Municipality}
 */
function isMunicipality(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    "id" in value &&
    typeof value.id === "string" &&
    "name" in value &&
    typeof value.name === "string" &&
    "province" in value &&
    typeof value.province === "string" &&
    "timezone" in value &&
    typeof value.timezone === "string" &&
    supportedTimezones.has(value.timezone)
  );
}

/**
 * @param {string} value
 * @returns {string}
 */
export function normalizeSearchText(value) {
  return value
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLocaleLowerCase("es")
    .trim()
    .replace(/\s+/gu, " ");
}

/**
 * @param {Municipality[]} municipalities
 * @param {string} query
 * @returns {Municipality[]}
 */
export function searchMunicipalities(municipalities, query) {
  return municipalities
    .filter((municipality) => municipality.searchName?.includes(query))
    .sort((left, right) => {
      const prefixDifference =
        Number(right.searchName?.startsWith(query)) -
        Number(left.searchName?.startsWith(query));
      return prefixDifference || compareMunicipalities(left, right);
    });
}

/**
 * @param {Set<string>} trackedIds
 * @param {Map<string, Municipality>} municipalitiesById
 * @returns {Municipality[]}
 */
export function getTrackedMunicipalities(trackedIds, municipalitiesById) {
  return [...trackedIds]
    .map((id) => municipalitiesById.get(id))
    .filter((municipality) => municipality !== undefined);
}
