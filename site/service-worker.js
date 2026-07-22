const cachePrefix = "cielo-";
// Bump the shell version whenever a release changes the precached application.
const shellCacheName = `${cachePrefix}shell-v1`;
const weatherCacheName = `${cachePrefix}weather-v1`;
const shellPaths = [
  "./",
  "./index.html",
  "./icon.svg",
  "./assets/site.css",
  "./assets/site.js",
  "./assets/components/cielo-app.js",
  "./assets/components/cielo-icon.js",
  "./assets/components/cielo-locations-view.js",
  "./assets/components/cielo-municipality-row.js",
  "./assets/components/cielo-municipality-view.js",
  "./assets/lib/catalog.js",
  "./assets/lib/storage.js",
  "./assets/lib/weather.js",
  "./assets/icons/circle-x.svg",
  "./assets/icons/cloud-drizzle.svg",
  "./assets/icons/cloud-fog.svg",
  "./assets/icons/cloud-lightning.svg",
  "./assets/icons/cloud-moon-rain.svg",
  "./assets/icons/cloud-moon.svg",
  "./assets/icons/cloud-rain.svg",
  "./assets/icons/cloud-snow.svg",
  "./assets/icons/cloud-sun-rain.svg",
  "./assets/icons/cloud-sun.svg",
  "./assets/icons/cloud.svg",
  "./assets/icons/cloudy.svg",
  "./assets/icons/list.svg",
  "./assets/icons/map-pin.svg",
  "./assets/icons/moon.svg",
  "./assets/icons/search.svg",
  "./assets/icons/snowflake.svg",
  "./assets/icons/sun.svg",
  "./assets/icons/trash-2.svg",
];
const catalogPath = "./data/municipalities.json";

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

  event.respondWith(networkFirst(request));
});

async function installCaches() {
  const scope = self.registration.scope;
  const shellCache = await caches.open(shellCacheName);
  const weatherCache = await caches.open(weatherCacheName);

  // Seed everything needed to reopen the application and discover saved places.
  await Promise.all([
    shellCache.addAll(
      shellPaths.map((path) =>
        new Request(new URL(path, scope), { cache: "reload" })
      ),
    ),
    weatherCache.add(
      new Request(new URL(catalogPath, scope), { cache: "reload" }),
    ),
  ]);
  await self.skipWaiting();
}

async function activateCaches() {
  const currentCaches = new Set([shellCacheName, weatherCacheName]);
  const cacheNames = await caches.keys();

  // Retire only caches owned by Cielo while preserving the current weather data.
  await Promise.all(
    cacheNames
      .filter((name) => name.startsWith(cachePrefix) && !currentCaches.has(name))
      .map((name) => caches.delete(name)),
  );
  await self.clients.claim();
}

/** @param {Request} request */
async function networkFirst(request) {
  const cache = await caches.open(cacheNameFor(request.url));
  const revalidationRequest = new Request(request, { cache: "no-cache" });

  try {
    // Stable URLs let the HTTP cache turn unchanged ETags into body-free revalidation.
    const response = await fetch(revalidationRequest);
    if (response.ok) {
      try {
        await cache.put(request, response.clone());
      } catch (error) {
        // Offline storage is optional; never discard a usable network response.
        console.warn("No se pudo guardar la respuesta sin conexión", error);
      }
      return response;
    }

    return await cache.match(request) ?? response;
  } catch (error) {
    const cachedResponse = await cache.match(request);
    if (cachedResponse !== undefined) {
      return cachedResponse;
    }

    if (request.mode === "navigate") {
      const shellCache = await caches.open(shellCacheName);
      const fallback = await shellCache.match(
        new URL("./index.html", self.registration.scope),
      );
      if (fallback !== undefined) {
        return fallback;
      }
    }

    throw error;
  }
}

/** @param {string} requestUrl */
function cacheNameFor(requestUrl) {
  const dataUrl = new URL("./data/", self.registration.scope);
  return requestUrl.startsWith(dataUrl.href)
    ? weatherCacheName
    : shellCacheName;
}
