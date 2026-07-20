export const lastOpenedMunicipalityStorageKey = "cielo.lastMunicipality";
export const trackedMunicipalitiesStorageKey = "cielo.trackedMunicipalities";

/**
 * @param {Storage | null} [storage]
 * @returns {string | null}
 */
export function readLastOpenedMunicipalityId(storage = null) {
  try {
    const targetStorage = storage ?? window.localStorage;
    const municipalityId = targetStorage.getItem(
      lastOpenedMunicipalityStorageKey,
    );
    return municipalityId === null || municipalityId.length === 0
      ? null
      : municipalityId;
  } catch (error) {
    console.error("No se pudo leer el último municipio abierto", error);
    return null;
  }
}

/**
 * @param {string | null} municipalityId
 * @param {Storage | null} [storage]
 */
export function saveLastOpenedMunicipalityId(
  municipalityId,
  storage = null,
) {
  try {
    const targetStorage = storage ?? window.localStorage;
    if (municipalityId === null) {
      targetStorage.removeItem(lastOpenedMunicipalityStorageKey);
      return;
    }

    targetStorage.setItem(lastOpenedMunicipalityStorageKey, municipalityId);
  } catch (error) {
    console.error("No se pudo guardar el último municipio abierto", error);
  }
}

/**
 * @param {Storage | null} [storage]
 * @returns {Set<string>}
 */
export function readTrackedMunicipalityIds(storage = null) {
  try {
    const targetStorage = storage ?? window.localStorage;
    const storedIds = JSON.parse(
      targetStorage.getItem(trackedMunicipalitiesStorageKey) ?? "[]",
    );
    if (!Array.isArray(storedIds) || !storedIds.every(isString)) {
      return new Set();
    }

    return new Set(storedIds);
  } catch (error) {
    console.error("No se pudieron leer los municipios guardados", error);
    return new Set();
  }
}

/**
 * @param {Set<string>} trackedIds
 * @param {Storage | null} [storage]
 */
export function saveTrackedMunicipalityIds(
  trackedIds,
  storage = null,
) {
  try {
    const targetStorage = storage ?? window.localStorage;
    targetStorage.setItem(
      trackedMunicipalitiesStorageKey,
      JSON.stringify([...trackedIds]),
    );
  } catch (error) {
    console.error("No se pudieron guardar los municipios", error);
  }
}

/** @param {unknown} value */
function isString(value) {
  return typeof value === "string";
}
