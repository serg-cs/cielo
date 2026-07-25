export const lastOpenedMunicipalityStorageKey = "cielo.lastOpenedMunicipalityId";
export const savedMunicipalityIdsStorageKey = "cielo.savedMunicipalityIds";

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
 * Read saved municipality IDs in their user-defined order.
 *
 * @param {Storage | null} [storage]
 * @returns {Set<string>}
 */
export function readSavedMunicipalityIds(storage = null) {
  try {
    const targetStorage = storage ?? window.localStorage;
    const storedIds = JSON.parse(
      targetStorage.getItem(savedMunicipalityIdsStorageKey) ?? "[]",
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
 * Save municipality IDs in their user-defined order.
 *
 * @param {Set<string>} savedMunicipalityIds
 * @param {Storage | null} [storage]
 */
export function saveSavedMunicipalityIds(
  savedMunicipalityIds,
  storage = null,
) {
  try {
    const targetStorage = storage ?? window.localStorage;
    targetStorage.setItem(
      savedMunicipalityIdsStorageKey,
      JSON.stringify([...savedMunicipalityIds]),
    );
  } catch (error) {
    console.error("No se pudieron guardar los municipios", error);
  }
}

/** @param {unknown} value */
function isString(value) {
  return typeof value === "string";
}
