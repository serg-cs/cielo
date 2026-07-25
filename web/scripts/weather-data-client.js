export const weatherDataCacheName = "cielo-weather-data";

/**
 * Read and validate one previously stored JSON response.
 *
 * @template T
 * @param {URL} url
 * @param {(document: unknown) => T} validate
 * @returns {Promise<T | null>}
 */
export async function readValidatedJson(url, validate) {
  if (!("caches" in globalThis)) {
    return null;
  }

  let cache;
  let response;
  try {
    cache = await caches.open(weatherDataCacheName);
    response = await cache.match(url);
  } catch (error) {
    console.warn("No se pudieron leer los datos guardados", error);
    return null;
  }
  if (response === undefined) {
    return null;
  }

  try {
    return validate(await response.json());
  } catch (error) {
    // Never reuse a response that no longer satisfies the application schema.
    console.warn("Se descartaron datos guardados no válidos", error);
    try {
      await cache.delete(url);
    } catch (deleteError) {
      console.warn("No se pudieron descartar los datos no válidos", deleteError);
    }
    return null;
  }
}

/**
 * Fetch and validate JSON before replacing the last-known-good response.
 *
 * @template T
 * @param {URL} url
 * @param {(document: unknown) => T} validate
 * @param {typeof fetch} [fetcher]
 * @returns {Promise<T>}
 */
export async function fetchValidatedJson(
  url,
  validate,
  fetcher = globalThis.fetch.bind(globalThis),
) {
  const response = await fetcher(url, { cache: "no-cache" });
  if (!response.ok) {
    throw new Error(`No se pudieron cargar los datos: HTTP ${response.status}`);
  }

  // Keep the original body available for Cache Storage while validating a clone.
  const value = validate(await response.clone().json());
  if (!("caches" in globalThis)) {
    return value;
  }

  try {
    const cache = await caches.open(weatherDataCacheName);
    await cache.put(url, response);
  } catch (error) {
    // Persistence is optional once a usable network response has been validated.
    console.warn("No se pudieron guardar los datos para usarlos sin conexión", error);
  }
  return value;
}
