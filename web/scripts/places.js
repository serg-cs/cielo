/**
 * Places: municipalities and beaches, paths to their data files and search.
 */

const BUNDLE_SIZE = 20;
const MAXIMUM_RESULTS = 50;

export const PLACES_PATH = "v4/places.json";
/** When the data was last requested from AEMET. */
export const BUILD_PATH = "v4/build.json";
export const WARNINGS_PATH = "v4/warnings.json";

/** @param {string} id Municipality INE code such as `28079`. */
export function forecastPath(id) {
  const start = Math.floor(Number(id.slice(2, 5)) / BUNDLE_SIZE) * BUNDLE_SIZE;
  return `v4/forecast/${id.slice(0, 2)}/${String(start).padStart(3, "0")}.json`;
}

/** @param {string} id Municipality or beach ID; both start with the province. */
export const beachesPath = (id) => `v4/beaches/${id.slice(0, 2)}.json`;

/** @param {string} id */
export const isBeachId = (id) => /^\d{7}$/.test(id);

/**
 * Time zone of a municipality or beach, from its province.
 *
 * @param {string} id
 */
export function timeZoneFor(id) {
  const province = id.slice(0, 2);
  if (province === "35" || province === "38") return "Atlantic/Canary";
  if (province === "51" || province === "52") return "Africa/Ceuta";
  return "Europe/Madrid";
}

/**
 * @typedef {object} Place
 * @property {"municipality"} kind
 * @property {string} id
 * @property {string} name
 * @property {string} province
 * @property {string} timeZone
 * @property {number} population
 * @property {string} key Normalized name for search.
 * @property {string} provinceKey Normalized province name.
 */

/**
 * @typedef {object} Beach
 * @property {"beach"} kind
 * @property {string} id
 * @property {string} name
 * @property {string} municipalityId
 * @property {string} municipality Municipality name.
 * @property {string} province
 * @property {string} timeZone
 * @property {string} key
 * @property {string} municipalityKey
 */

/**
 * Decode the municipalities of `places.json`.
 *
 * @param {any} document
 * @returns {Map<string, Place>}
 */
export function decodePlaces(document) {
  /** @type {Map<string, {name: string, key: string}>} */
  const provinces = new Map();
  for (const province of document?.provinces ?? []) {
    provinces.set(province.code, {
      name: province.name,
      key: normalize(province.name),
    });
  }
  /** @type {Map<string, Place>} */
  const places = new Map();
  const ids = Array.isArray(document?.ids) ? document.ids : [];
  ids.forEach((id, index) => {
    const name = document.names?.[index];
    if (typeof id !== "string" || typeof name !== "string") return;
    places.set(id, {
      kind: "municipality",
      id,
      name,
      province: provinces.get(id.slice(0, 2))?.name ?? "",
      timeZone: timeZoneFor(id),
      population: Number(document.pop?.[index] ?? 0),
      key: normalize(name),
      provinceKey: provinces.get(id.slice(0, 2))?.key ?? "",
    });
  });
  return places;
}

/**
 * Decode the beaches of `places.json`.
 *
 * @param {any} document
 * @param {Map<string, Place>} places
 * @returns {Map<string, Beach>}
 */
export function decodeBeaches(document, places) {
  const columns = document?.beaches ?? {};
  /** @type {Map<string, Beach>} */
  const beaches = new Map();
  const ids = Array.isArray(columns.ids) ? columns.ids : [];
  ids.forEach((id, index) => {
    const name = columns.names?.[index];
    const municipalityId = columns.m?.[index];
    const municipality = places.get(municipalityId);
    if (typeof id !== "string" || typeof name !== "string" || !municipality) return;
    beaches.set(id, {
      kind: "beach",
      id,
      name,
      municipalityId,
      municipality: municipality.name,
      province: municipality.province,
      timeZone: municipality.timeZone,
      key: normalize(name),
      municipalityKey: municipality.key,
    });
  });
  return beaches;
}

/**
 * Lowercase text without accents or extra spaces.
 *
 * @param {string} text
 */
export function normalize(text) {
  return text
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase()
    .replace(/[^\p{Letter}\p{Number}]+/gu, " ")
    .trim();
}

/**
 * How well a name matches a query, lower is better, -1 for no match:
 * 0 exact, 1 starts with it, 2 a word starts with it, 3 every word of the
 * query starts a word of the name (any order), 4 contained anywhere,
 * including when spaces and punctuation are ignored (`lhospitalet`).
 *
 * @param {string} key Normalized name.
 * @param {string} needle Normalized query.
 */
function nameRank(key, needle) {
  if (key === needle) return 0;
  if (key.startsWith(needle)) return 1;
  if (key.includes(` ${needle}`)) return 2;
  const tokens = needle.split(" ");
  if (tokens.length > 1) {
    const words = key.split(" ");
    if (tokens.every((token) => words.some((word) => word.startsWith(token)))) return 3;
  }
  if (key.includes(needle) || key.replaceAll(" ", "").includes(needle.replaceAll(" ", ""))) return 4;
  return -1;
}

/**
 * Search municipalities and beaches by name. Better name matches first,
 * municipalities before beaches, then larger populations. Beaches also
 * match their municipality's name, and municipalities their province's,
 * after every direct name match.
 *
 * @param {Map<string, Place>} places
 * @param {string} query
 * @param {Map<string, Beach>} [beaches]
 * @returns {{results: Array<Place | Beach>, total: number}}
 */
export function search(places, query, beaches = new Map()) {
  const needle = normalize(query);
  if (needle.length === 0) {
    return { results: [], total: 0 };
  }
  /** @type {Array<{place: Place | Beach, rank: number, weight: number}>} */
  const matches = [];
  for (const place of places.values()) {
    let rank = nameRank(place.key, needle);
    if (rank < 0 && needle.length >= 3 && place.provinceKey.startsWith(needle)) rank = 6;
    if (rank >= 0) matches.push({ place, rank, weight: place.population });
  }
  for (const beach of beaches.values()) {
    let rank = nameRank(beach.key, needle);
    if (rank >= 0) rank += 0.5;
    else if (
      needle.length >= 3 &&
      nameRank(beach.municipalityKey, needle) >= 0 &&
      nameRank(beach.municipalityKey, needle) <= 2
    )
      rank = 5;
    if (rank >= 0) matches.push({ place: beach, rank, weight: 0 });
  }
  matches.sort(
    (left, right) =>
      left.rank - right.rank || right.weight - left.weight || left.place.name.localeCompare(right.place.name, "es"),
  );
  return {
    results: matches.slice(0, MAXIMUM_RESULTS).map((match) => match.place),
    total: matches.length,
  };
}

/**
 * The most populated municipalities, as suggestions.
 *
 * @param {Map<string, Place>} places
 * @param {number} count
 */
export function largestPlaces(places, count) {
  return [...places.values()].sort((left, right) => right.population - left.population).slice(0, count);
}
