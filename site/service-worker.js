const cachePrefix = "cielo-";
const dataUrl = new URL(
  "./data/" /* @cielo-data-url */,
  self.registration.scope,
);
const municipalitiesUrl = new URL("municipalities.json", dataUrl);
const temperaturesUrl = new URL("temperatures/", dataUrl);
// Bump the shell version whenever a release changes the precached application.
const shellCacheName = `${cachePrefix}shell-v1`;
// Application code owns this cache and writes only schema-validated responses.
const dataCacheName = `${cachePrefix}data-v1`;
const shellPaths = [
  "./",
  "./index.html",
  "./manifest.webmanifest",
  "./icon.svg",
  "./assets/app-icons/apple-touch-icon.png",
  "./assets/app-icons/icon-192.png",
  "./assets/app-icons/icon-512.png",
  "./assets/app-icons/icon-maskable-512.png",
  "./assets/site.css",
  "./assets/site.js",
  "./assets/components/cielo-app.js",
  "./assets/components/cielo-icon.js",
  "./assets/components/cielo-locations-view.js",
  "./assets/components/cielo-municipality-row.js",
  "./assets/components/cielo-municipality-view.js",
  "./assets/lib/catalog.js",
  "./assets/lib/config.js",
  "./assets/lib/data-cache.js",
  "./assets/lib/storage.js",
  "./assets/lib/weather.js",
];

self.addEventListener("install", (event) => {
  event.waitUntil(installCaches());
});

self.addEventListener("activate", (event) => {
  event.waitUntil(activateCaches());
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== self.location.origin) {
    return;
  }
  if (
    url.href === municipalitiesUrl.href ||
    url.href.startsWith(temperaturesUrl.href)
  ) {
    // The page validates data before replacing its last-known-good response.
    return;
  }

  event.respondWith(cacheFirst(request));
});

async function installCaches() {
  const scope = self.registration.scope;
  const shellCache = await caches.open(shellCacheName);

  // Seed the complete immutable application shell in one cache operation.
  await shellCache.addAll(
    shellPaths.map((path) => new Request(new URL(path, scope), { cache: "reload" })),
  );
  await self.skipWaiting();
}

async function activateCaches() {
  const currentCaches = new Set([shellCacheName, dataCacheName]);
  const cacheNames = await caches.keys();

  // Retire only caches owned by Cielo while preserving validated application data.
  await Promise.all(
    cacheNames
      .filter((name) => name.startsWith(cachePrefix) && !currentCaches.has(name))
      .map((name) => caches.delete(name)),
  );
  await self.clients.claim();
}

/** @param {Request} request */
async function cacheFirst(request) {
  const cache = await caches.open(shellCacheName);
  const cachedResponse = await cache.match(request);
  if (cachedResponse !== undefined) {
    return cachedResponse;
  }

  try {
    const response = await fetch(request);
    if (response.ok) {
      try {
        await cache.put(request, response.clone());
      } catch (error) {
        // Offline storage is optional; never discard a usable network response.
        console.warn("No se pudo guardar la respuesta sin conexión", error);
      }
      return response;
    }

    return response;
  } catch (error) {
    if (request.mode === "navigate") {
      const fallback = await cache.match(new URL("./index.html", self.registration.scope));
      if (fallback !== undefined) {
        return fallback;
      }
    }

    throw error;
  }
}
