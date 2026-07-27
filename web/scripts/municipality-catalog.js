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
 */

/**
 * @typedef {object} PublishedProvince
 * @property {string} name
 * @property {string} tz
 * @property {PublishedMunicipality[]} municipalities
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
    !("updated_at" in document) ||
    typeof document.updated_at !== "string" ||
    !sourceGenerationTimePattern.test(document.updated_at) ||
    !("provinces" in document) ||
    !Array.isArray(document.provinces) ||
    document.provinces.length === 0 ||
    !document.provinces.every(isProvince)
  ) {
    throw new Error("El documento de municipios no es válido");
  }

  // Expand shared province data while rejecting ambiguous identifiers.
  const provinceNames = new Set();
  const municipalityIds = new Set();
  const municipalities = [];
  for (const province of document.provinces) {
    if (provinceNames.has(province.name)) {
      throw new Error("El documento de municipios no es válido");
    }
    provinceNames.add(province.name);

    for (const municipality of province.municipalities) {
      if (municipalityIds.has(municipality.id)) {
        throw new Error("El documento de municipios no es válido");
      }
      municipalityIds.add(municipality.id);
      municipalities.push({
        id: municipality.id,
        name: municipality.name,
        province: province.name,
        timeZone: province.tz,
        searchName: normalizeSearchText(municipality.name),
      });
    }
  }

  return {
    municipalities,
    generatedAt: document.updated_at,
  };
}

/**
 * @param {unknown} value
 * @returns {value is PublishedProvince}
 */
function isProvince(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    "name" in value &&
    typeof value.name === "string" &&
    value.name.trim().length > 0 &&
    "tz" in value &&
    typeof value.tz === "string" &&
    supportedTimezones.has(value.tz) &&
    "municipalities" in value &&
    Array.isArray(value.municipalities) &&
    value.municipalities.length > 0 &&
    value.municipalities.every(isMunicipality)
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
    /^\d{5}$/u.test(value.id) &&
    "name" in value &&
    typeof value.name === "string" &&
    value.name.trim().length > 0
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
