// Cielo service worker: keeps the app shell available offline.
// The build replaces the version and the list of files below.
const VERSION = "__CIELO_VERSION__";
const PRECACHE = /* __CIELO_PRECACHE__ */ [];
const CACHE = `cielo-app-${VERSION}`;
const NAVIGATION_TIMEOUT_MS = 3000;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(CACHE)
      .then((cache) => cache.addAll(PRECACHE))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys.filter((key) => key.startsWith("cielo-app-") && key !== CACHE).map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  if (request.method !== "GET") return;
  const url = new URL(request.url);
  // Data lives on another origin or path and is cached by the page itself.
  if (url.origin !== self.location.origin) return;

  if (request.mode === "navigate") {
    event.respondWith(networkFirst(request));
    return;
  }
  if (PRECACHE.some((path) => new URL(path, self.location.href).href === url.href)) {
    event.respondWith(caches.match(request).then((cached) => cached ?? fetch(request)));
  }
});

/**
 * The latest page when the network answers quickly, otherwise the cached one.
 *
 * @param {Request} request
 */
async function networkFirst(request) {
  const cache = await caches.open(CACHE);
  try {
    const response = await Promise.race([
      fetch(request),
      new Promise((_, reject) => setTimeout(() => reject(new Error("timeout")), NAVIGATION_TIMEOUT_MS)),
    ]);
    if (response.ok) {
      await cache.put("./", response.clone());
    }
    return response;
  } catch {
    const cached = (await cache.match("./")) ?? (await cache.match(request));
    if (cached) return cached;
    throw new Error("offline and no cached page");
  }
}
