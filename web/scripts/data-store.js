/**
 * Loads published data files, keeping the last valid copy in Cache Storage
 * so the app works offline.
 */

const CACHE_NAME = "cielo-data-v4";
const REQUEST_TIMEOUT_MS = 10000;
const SCHEMA = 4;

/**
 * @typedef {object} Entry
 * @property {unknown} data Last valid document, or `null`.
 * @property {number} fetchedAt When `data` was last confirmed from the network (ms), or 0.
 * @property {"idle" | "loading" | "ready" | "missing" | "error" | "offline"} status
 */

export class DataStore extends EventTarget {
  /** @param {string} baseUrl Absolute URL of the data root, ending in `/`. */
  constructor(baseUrl) {
    super();
    this.baseUrl = baseUrl;
    /** @type {Map<string, Entry>} */
    this.entries = new Map();
    /** @type {Map<string, Promise<void>>} */
    this.pending = new Map();
  }

  /**
   * Current entry for a path.
   *
   * @param {string} path Path relative to the data root, such as `v4/places.json`.
   * @returns {Entry}
   */
  peek(path) {
    return this.entries.get(path) ?? { data: null, fetchedAt: 0, status: "idle" };
  }

  /**
   * Load a file: the cached copy first, then the network when older than `maxAge`.
   *
   * @param {string} path
   * @param {{maxAge?: number, force?: boolean}} [options]
   * @returns {Promise<unknown>} The best available document, or `null`.
   */
  async load(path, { maxAge = 10 * 60 * 1000, force = false } = {}) {
    if (!this.entries.has(path)) {
      this.update(path, { status: "loading" });
      const cached = await readCache(this.url(path));
      if (cached && isValid(cached.data)) {
        this.update(path, {
          data: cached.data,
          fetchedAt: cached.fetchedAt,
          status: "ready",
        });
      }
    }
    const entry = this.peek(path);
    const stale = force || Date.now() - entry.fetchedAt > maxAge;
    if (stale) {
      const request = this.pending.get(path) ?? this.fetch(path);
      this.pending.set(path, request);
      if (entry.data === null) {
        await request;
      }
    }
    return this.peek(path).data;
  }

  /**
   * Refresh every loaded file older than `maxAge`.
   *
   * @param {number} maxAge
   */
  refreshAll(maxAge) {
    for (const path of this.entries.keys()) {
      void this.load(path, { maxAge });
    }
  }

  /** @param {string} path */
  url(path) {
    return new URL(path, this.baseUrl).href;
  }

  /** @param {string} path */
  async fetch(path) {
    try {
      if (navigator.onLine === false) {
        this.update(path, { status: "offline" });
        return;
      }
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), REQUEST_TIMEOUT_MS);
      let response;
      try {
        response = await fetch(this.url(path), {
          cache: "no-cache",
          signal: controller.signal,
        });
      } finally {
        clearTimeout(timer);
      }
      if (response.status === 404) {
        this.update(path, { status: "missing", fetchedAt: Date.now() });
        return;
      }
      if (!response.ok) {
        throw new Error(`HTTP ${response.status}`);
      }
      const data = await response.json();
      if (!isValid(data)) {
        throw new Error(`invalid data in ${path}`);
      }
      const fetchedAt = Date.now();
      this.update(path, { data, fetchedAt, status: "ready" });
      await writeCache(this.url(path), data, fetchedAt);
    } catch (error) {
      console.warn(`Cielo: could not load ${path}`, error);
      this.update(path, {
        status: navigator.onLine === false ? "offline" : "error",
      });
    } finally {
      this.pending.delete(path);
    }
  }

  /**
   * @param {string} path
   * @param {Partial<Entry>} changes
   */
  update(path, changes) {
    this.entries.set(path, { ...this.peek(path), ...changes });
    this.dispatchEvent(new CustomEvent("change", { detail: { path } }));
  }
}

/** @param {unknown} data */
function isValid(data) {
  return typeof data === "object" && data !== null && /** @type {{schema?: number}} */ (data).schema === SCHEMA;
}

/**
 * @param {string} url
 * @returns {Promise<{data: unknown, fetchedAt: number} | null>}
 */
async function readCache(url) {
  try {
    if (!("caches" in globalThis)) return null;
    const cache = await caches.open(CACHE_NAME);
    const response = await cache.match(url);
    if (!response) return null;
    const fetchedAt = Number(response.headers.get("x-cielo-fetched-at") ?? 0);
    return { data: await response.json(), fetchedAt };
  } catch {
    return null;
  }
}

/**
 * @param {string} url
 * @param {unknown} data
 * @param {number} fetchedAt
 */
async function writeCache(url, data, fetchedAt) {
  try {
    if (!("caches" in globalThis)) return;
    const cache = await caches.open(CACHE_NAME);
    await cache.put(
      url,
      new Response(JSON.stringify(data), {
        headers: {
          "content-type": "application/json",
          "x-cielo-fetched-at": String(fetchedAt),
        },
      }),
    );
  } catch (error) {
    console.warn("Cielo: could not cache data", error);
  }
}
