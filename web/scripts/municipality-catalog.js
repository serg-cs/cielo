const collator = new Intl.Collator("es", {
  numeric: true,
  sensitivity: "base",
});
const generatorIdentity = "cielo";
const sourceGenerationTimePattern =
  /^\d{4}-\d{2}-\d{2}T(?:[01]\d|2[0-3]):[0-5]\d(?::[0-5]\d(?:\.\d+)?)?(?:Z|[+-]\d{2}:\d{2})?$/u;
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
 * @property {string} timeZone
 * @property {string} [searchName]
 */

/**
 * @typedef {object} PublishedMunicipality
 * @property {string} id
 * @property {string} name
 * @property {string} province
 * @property {string} time_zone
 */

/**
 * @typedef {object} MunicipalityCatalog
 * @property {Municipality[]} municipalities
 * @property {string} generatedAt
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
 * @returns {MunicipalityCatalog}
 */
export function validateMunicipalityCatalog(document) {
  if (
    typeof document !== "object" ||
    document === null ||
    !("generator" in document) ||
    document.generator !== generatorIdentity ||
    !("municipalities" in document) ||
    !Array.isArray(document.municipalities) ||
    !document.municipalities.every(isMunicipality) ||
    !("source" in document) ||
    !isCatalogSource(document.source)
  ) {
    throw new Error("El documento de municipios no es válido");
  }

  return {
    municipalities: document.municipalities.map((municipality) => ({
      id: municipality.id,
      name: municipality.name,
      province: municipality.province,
      timeZone: municipality.time_zone,
      searchName: normalizeSearchText(municipality.name),
    })),
    generatedAt: document.source.generated_at,
  };
}

/** @param {unknown} value */
function isCatalogSource(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    value.name === "AEMET" &&
    "url" in value &&
    value.url === "https://opendata.aemet.es/" &&
    "generated_at" in value &&
    typeof value.generated_at === "string" &&
    sourceGenerationTimePattern.test(value.generated_at)
  );
}

/**
 * @param {unknown} value
 * @returns {value is PublishedMunicipality}
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
    "time_zone" in value &&
    typeof value.time_zone === "string" &&
    supportedTimezones.has(value.time_zone)
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
 * @param {Set<string>} savedMunicipalityIds
 * @param {Map<string, Municipality>} municipalitiesById
 * @returns {Municipality[]}
 */
export function getSavedMunicipalities(
  savedMunicipalityIds,
  municipalitiesById,
) {
  return [...savedMunicipalityIds]
    .map((id) => municipalitiesById.get(id))
    .filter((municipality) => municipality !== undefined);
}
