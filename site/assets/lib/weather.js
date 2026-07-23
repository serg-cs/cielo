import {
  fetchValidatedJson,
  readValidatedJson,
} from "./data-cache.js";

const forecastSchemaVersion = 1;
const hourlyForecastPeriodCount = 24;
const refreshIntervalMilliseconds = 30 * 60 * 1_000;
const hourMilliseconds = 60 * 60 * 1_000;
const temperatureMinimum = -32_768;
const temperatureMaximum = 32_767;
const supportedSkyStates = new Set([
  "cloud",
  "cloud-drizzle",
  "cloud-fog",
  "cloud-lightning",
  "cloud-moon",
  "cloud-moon-rain",
  "cloud-rain",
  "cloud-snow",
  "cloud-sun",
  "cloud-sun-rain",
  "cloudy",
  "moon",
  "snowflake",
  "sun",
]);

/** @typedef {import("./catalog.js").Municipality} Municipality */
/** @typedef {"loading" | "ready" | "offline" | "error"} ForecastStatus */

/**
 * @typedef {object} CurrentForecast
 * @property {number} celsius
 * @property {string} state
 * @property {string} description
 */

/**
 * @typedef {object} HourlyForecast
 * @property {string} municipalityId
 * @property {string} timezone
 * @property {Map<string, CurrentForecast>} forecastsByHour
 */

/**
 * @typedef {object} HourlyForecastPeriod
 * @property {number} hour
 * @property {CurrentForecast | null} forecast
 */

/**
 * @typedef {object} CurrentForecastChangeDetail
 * @property {string} municipalityId
 * @property {CurrentForecast | null} forecast
 */

/**
 * @param {unknown} document
 * @param {Municipality} municipality
 * @returns {HourlyForecast}
 */
export function validateForecastDocument(document, municipality) {
  if (
    typeof document !== "object" ||
    document === null ||
    !("schema_version" in document) ||
    document.schema_version !== forecastSchemaVersion ||
    !("municipality_id" in document) ||
    document.municipality_id !== municipality.id ||
    !("timezone" in document) ||
    document.timezone !== municipality.timezone ||
    !("temperatures" in document) ||
    !Array.isArray(document.temperatures) ||
    document.temperatures.length === 0
  ) {
    throw new Error("El documento de temperaturas no es válido");
  }

  // Index exact local hours while rejecting ambiguous forecast documents.
  const forecastsByHour = new Map();
  for (const hourlyForecast of document.temperatures) {
    if (!isHourlyForecast(hourlyForecast)) {
      throw new Error("El documento de temperaturas no es válido");
    }

    const key = forecastKey(hourlyForecast.date, hourlyForecast.hour);
    if (forecastsByHour.has(key)) {
      throw new Error("El documento de temperaturas contiene horas duplicadas");
    }
    forecastsByHour.set(key, {
      celsius: hourlyForecast.celsius,
      state: hourlyForecast.state,
      description: hourlyForecast.description,
    });
  }

  return {
    municipalityId: municipality.id,
    timezone: municipality.timezone,
    forecastsByHour,
  };
}

/**
 * @param {HourlyForecast} forecast
 * @param {Date} [now]
 * @returns {CurrentForecast | null}
 */
export function selectCurrentForecast(forecast, now = new Date()) {
  const key = currentForecastKey(now, forecast.timezone);
  return forecast.forecastsByHour.get(key) ?? null;
}

/**
 * @param {HourlyForecast} forecast
 * @param {Date} [now]
 * @returns {HourlyForecastPeriod[]}
 */
export function selectHourlyForecast(forecast, now = new Date()) {
  const currentHour = now.getTime() - now.getTime() % hourMilliseconds;

  // Walk real elapsed hours so local labels remain correct across day and DST changes.
  return Array.from({ length: hourlyForecastPeriodCount }, (_, index) => {
    const instant = new Date(currentHour + index * hourMilliseconds);
    const { key, hour } = forecastTime(instant, forecast.timezone);
    return {
      hour,
      forecast: forecast.forecastsByHour.get(key) ?? null,
    };
  });
}

/**
 * @param {CurrentForecast | null} currentForecast
 * @param {HourlyForecastPeriod[]} hourlyForecast
 * @returns {boolean}
 */
function forecastSelectionsAreUsable(currentForecast, hourlyForecast) {
  return currentForecast !== null ||
    hourlyForecast.some((period) => period.forecast !== null);
}

export class CurrentForecastStore extends EventTarget {
  #catalogById = new Map();
  #trackedIds = new Set();
  #forecasts = new Map();
  #currentForecasts = new Map();
  #forecastStatuses = new Map();
  #inFlight = new Map();
  #fetcher;
  #now;
  #running = false;
  #refreshTimeoutId = null;
  #hourTimeoutId = null;

  /**
   * @param {{fetcher?: typeof fetch, now?: () => Date}} [options]
   */
  constructor({
    fetcher = globalThis.fetch.bind(globalThis),
    now = () => new Date(),
  } = {}) {
    super();
    this.#fetcher = fetcher;
    this.#now = now;
  }

  /**
   * @param {Municipality[]} municipalities
   * @param {Set<string>} trackedIds
   */
  start(municipalities, trackedIds) {
    this.#catalogById = new Map(
      municipalities.map((municipality) => [municipality.id, municipality]),
    );
    this.setTrackedIds(trackedIds);
    if (!this.#running) {
      this.#running = true;
      window.addEventListener("online", this.#handleOnline);
      window.addEventListener("offline", this.#handleOffline);
      document.addEventListener("visibilitychange", this.#handleVisibilityChange);
    }

    // Recompute memory state, then hydrate from storage before using the network.
    this.#recomputeForecastSelections();
    void this.#hydrateAndRefreshIds([...this.#trackedIds]);
    this.#scheduleRefresh();
    this.#scheduleHourBoundary();
  }

  stop() {
    if (!this.#running) {
      return;
    }

    this.#running = false;
    window.removeEventListener("online", this.#handleOnline);
    window.removeEventListener("offline", this.#handleOffline);
    document.removeEventListener(
      "visibilitychange",
      this.#handleVisibilityChange,
    );
    this.#clearTimers();
  }

  /** @param {Set<string>} trackedIds */
  setTrackedIds(trackedIds) {
    const nextIds = new Set(
      [...trackedIds].filter((id) => this.#catalogById.has(id)),
    );
    const addedIds = [...nextIds].filter((id) => !this.#trackedIds.has(id));

    // Release in-memory data for locations that no longer participate in the UI.
    for (const id of this.#trackedIds) {
      if (!nextIds.has(id)) {
        this.#forecasts.delete(id);
        this.#forecastStatuses.delete(id);
        this.#publishCurrentForecast(id, null);
        this.#publishHourlyForecast(id, []);
      }
    }
    this.#trackedIds = nextIds;
    for (const id of addedIds) {
      this.#publishForecastStatus(
        id,
        this.#hasUsableForecast(id) ? "ready" : "loading",
      );
    }
    this.#recomputeForecastSelections();

    if (this.#running && addedIds.length > 0) {
      void this.#hydrateAndRefreshIds(addedIds);
    }
  }

  /** @param {string} municipalityId @returns {CurrentForecast | null} */
  getCurrentForecast(municipalityId) {
    return this.#currentForecasts.get(municipalityId) ?? null;
  }

  /** @param {string} municipalityId @returns {HourlyForecastPeriod[]} */
  getHourlyForecast(municipalityId) {
    const forecast = this.#forecasts.get(municipalityId);
    return forecast === undefined
      ? []
      : selectHourlyForecast(forecast, this.#now());
  }

  /** @param {string} municipalityId @returns {ForecastStatus} */
  getForecastStatus(municipalityId) {
    const status = this.#forecastStatuses.get(municipalityId);
    const usable = this.#hasUsableForecast(municipalityId);
    if (usable) {
      return "ready";
    }
    if (status === "ready") {
      return navigator.onLine ? "loading" : "offline";
    }
    return status ?? "loading";
  }

  async refreshNow() {
    await this.#refreshIds([...this.#trackedIds]);
  }

  /** @param {string[]} municipalityIds */
  async #hydrateAndRefreshIds(municipalityIds) {
    await Promise.all(
      municipalityIds.map((municipalityId) =>
        this.#hydrateAndRefreshMunicipality(municipalityId)
      ),
    );
  }

  /** @param {string} municipalityId */
  async #hydrateAndRefreshMunicipality(municipalityId) {
    const municipality = this.#catalogById.get(municipalityId);
    if (municipality === undefined || !this.#trackedIds.has(municipalityId)) {
      return;
    }

    if (!this.#forecasts.has(municipalityId)) {
      const forecast = await readValidatedJson(
        forecastUrl(municipality.id),
        (document) => validateForecastDocument(document, municipality),
      );
      if (
        forecast !== null &&
        this.#trackedIds.has(municipalityId) &&
        !this.#forecasts.has(municipalityId)
      ) {
        this.#storeForecast(municipalityId, forecast);
      }
    }
    if (!navigator.onLine) {
      if (!this.#hasUsableForecast(municipalityId)) {
        this.#publishForecastStatus(municipalityId, "offline");
      }
      return;
    }

    await this.#refreshMunicipality(municipalityId);
  }

  /** @param {string[]} municipalityIds */
  async #refreshIds(municipalityIds) {
    await Promise.all(
      municipalityIds.map((municipalityId) =>
        this.#refreshMunicipality(municipalityId)
      ),
    );
  }

  /** @param {string} municipalityId */
  async #refreshMunicipality(municipalityId) {
    const existingRequest = this.#inFlight.get(municipalityId);
    if (existingRequest !== undefined) {
      await existingRequest;
      return;
    }

    const municipality = this.#catalogById.get(municipalityId);
    if (municipality === undefined || !this.#trackedIds.has(municipalityId)) {
      return;
    }
    if (!navigator.onLine) {
      if (!this.#hasUsableForecast(municipalityId)) {
        this.#publishForecastStatus(municipalityId, "offline");
      }
      return;
    }
    if (!this.#hasUsableForecast(municipalityId)) {
      this.#publishForecastStatus(municipalityId, "loading");
    }

    const request = this.#loadForecast(municipality)
      .catch((error) => {
        if (!this.#hasUsableForecast(municipalityId)) {
          this.#publishForecastStatus(
            municipalityId,
            navigator.onLine ? "error" : "offline",
          );
        }
        console.error(
          `No se pudieron actualizar las temperaturas de ${municipality.name}`,
          error,
        );
      })
      .finally(() => {
        this.#inFlight.delete(municipalityId);
      });
    this.#inFlight.set(municipalityId, request);
    await request;
  }

  /** @param {Municipality} municipality */
  async #loadForecast(municipality) {
    const forecast = await fetchValidatedJson(
      forecastUrl(municipality.id),
      (document) => validateForecastDocument(document, municipality),
      this.#fetcher,
    );
    if (!this.#trackedIds.has(municipality.id)) {
      return;
    }

    if (!this.#storeForecast(municipality.id, forecast)) {
      throw new Error("La previsión no contiene horas vigentes");
    }
  }

  /**
   * @param {string} municipalityId
   * @param {HourlyForecast} forecast
   * @returns {boolean}
   */
  #storeForecast(municipalityId, forecast) {
    this.#forecasts.set(municipalityId, forecast);
    const now = this.#now();
    const currentForecast = selectCurrentForecast(forecast, now);
    const hourlyForecast = selectHourlyForecast(forecast, now);
    this.#publishCurrentForecast(
      municipalityId,
      currentForecast,
    );
    this.#publishHourlyForecast(
      municipalityId,
      hourlyForecast,
    );

    // A valid document is ready only while it covers a visible forecast period.
    const usable = forecastSelectionsAreUsable(
      currentForecast,
      hourlyForecast,
    );
    if (usable) {
      this.#publishForecastStatus(municipalityId, "ready");
    }
    return usable;
  }

  #recomputeForecastSelections() {
    const now = this.#now();
    for (const municipalityId of this.#trackedIds) {
      const forecast = this.#forecasts.get(municipalityId);
      if (forecast !== undefined) {
        const currentForecast = selectCurrentForecast(forecast, now);
        const hourlyForecast = selectHourlyForecast(forecast, now);
        this.#publishCurrentForecast(
          municipalityId,
          currentForecast,
        );
        this.#publishHourlyForecast(
          municipalityId,
          hourlyForecast,
        );

        // Reconcile status in both directions as the rolling window advances.
        if (forecastSelectionsAreUsable(currentForecast, hourlyForecast)) {
          this.#publishForecastStatus(municipalityId, "ready");
        } else if (this.#forecastStatuses.get(municipalityId) === "ready") {
          this.#publishForecastStatus(
            municipalityId,
            navigator.onLine ? "loading" : "offline",
          );
        }
      }
    }
  }

  /** @param {string} municipalityId @returns {boolean} */
  #hasUsableForecast(municipalityId) {
    const forecast = this.#forecasts.get(municipalityId);
    if (forecast === undefined) {
      return false;
    }

    const now = this.#now();
    return forecastSelectionsAreUsable(
      selectCurrentForecast(forecast, now),
      selectHourlyForecast(forecast, now),
    );
  }

  /** @param {string} municipalityId @param {CurrentForecast | null} forecast */
  #publishCurrentForecast(municipalityId, forecast) {
    const hasPrevious = this.#currentForecasts.has(municipalityId);
    const previous = this.#currentForecasts.get(municipalityId) ?? null;
    if (
      (!hasPrevious && forecast === null) ||
      currentForecastsAreEqual(previous, forecast)
    ) {
      return;
    }

    if (forecast === null) {
      this.#currentForecasts.delete(municipalityId);
    } else {
      this.#currentForecasts.set(municipalityId, forecast);
    }
    this.dispatchEvent(
      new CustomEvent("currentforecastchange", {
        detail: { municipalityId, forecast },
      }),
    );
  }

  /**
   * @param {string} municipalityId
   * @param {HourlyForecastPeriod[]} forecasts
   */
  #publishHourlyForecast(municipalityId, forecasts) {
    this.dispatchEvent(
      new CustomEvent("hourlyforecastchange", {
        detail: { municipalityId, forecasts },
      }),
    );
  }

  /** @param {string} municipalityId @param {ForecastStatus} status */
  #publishForecastStatus(municipalityId, status) {
    if (this.#forecastStatuses.get(municipalityId) === status) {
      return;
    }

    this.#forecastStatuses.set(municipalityId, status);
    this.dispatchEvent(
      new CustomEvent("forecaststatuschange", {
        detail: { municipalityId, status },
      }),
    );
  }

  #scheduleRefresh() {
    if (this.#refreshTimeoutId !== null) {
      window.clearTimeout(this.#refreshTimeoutId);
      this.#refreshTimeoutId = null;
    }
    if (!this.#running || document.visibilityState !== "visible") {
      return;
    }

    this.#refreshTimeoutId = window.setTimeout(() => {
      this.#refreshTimeoutId = null;
      void this.refreshNow();
      this.#scheduleRefresh();
    }, refreshIntervalMilliseconds);
  }

  #scheduleHourBoundary() {
    if (this.#hourTimeoutId !== null) {
      window.clearTimeout(this.#hourTimeoutId);
      this.#hourTimeoutId = null;
    }
    if (!this.#running || document.visibilityState !== "visible") {
      return;
    }

    const now = this.#now();
    const millisecondsUntilNextHour =
      hourMilliseconds - (now.getTime() % hourMilliseconds);
    this.#hourTimeoutId = window.setTimeout(() => {
      this.#hourTimeoutId = null;
      this.#recomputeForecastSelections();
      void this.refreshNow();
      this.#scheduleHourBoundary();
    }, millisecondsUntilNextHour + 50);
  }

  #clearTimers() {
    if (this.#refreshTimeoutId !== null) {
      window.clearTimeout(this.#refreshTimeoutId);
      this.#refreshTimeoutId = null;
    }
    if (this.#hourTimeoutId !== null) {
      window.clearTimeout(this.#hourTimeoutId);
      this.#hourTimeoutId = null;
    }
  }

  #handleOnline = () => {
    for (const municipalityId of this.#trackedIds) {
      if (!this.#hasUsableForecast(municipalityId)) {
        this.#publishForecastStatus(municipalityId, "loading");
      }
    }
    this.#recomputeForecastSelections();
    void this.refreshNow();
  };

  #handleOffline = () => {
    for (const municipalityId of this.#trackedIds) {
      if (!this.#hasUsableForecast(municipalityId)) {
        this.#publishForecastStatus(municipalityId, "offline");
      }
    }
  };

  #handleVisibilityChange = () => {
    if (document.visibilityState === "visible") {
      this.#recomputeForecastSelections();
      void this.refreshNow();
      this.#scheduleRefresh();
      this.#scheduleHourBoundary();
      return;
    }

    this.#clearTimers();
  };
}

/** @param {string} municipalityId */
function forecastUrl(municipalityId) {
  return new URL(
    `./data/temperatures/${encodeURIComponent(municipalityId)}.json`,
    document.baseURI,
  );
}

/** @param {unknown} value */
function isHourlyForecast(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    "date" in value &&
    typeof value.date === "string" &&
    isForecastDate(value.date) &&
    "hour" in value &&
    Number.isInteger(value.hour) &&
    value.hour >= 0 &&
    value.hour <= 23 &&
    "celsius" in value &&
    Number.isInteger(value.celsius) &&
    value.celsius >= temperatureMinimum &&
    value.celsius <= temperatureMaximum &&
    "state" in value &&
    typeof value.state === "string" &&
    supportedSkyStates.has(value.state) &&
    "description" in value &&
    typeof value.description === "string" &&
    value.description.trim().length > 0
  );
}

/** @param {string} value */
function isForecastDate(value) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/u.exec(value);
  if (match === null) {
    return false;
  }

  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const maximumDay = maximumDayOfMonth(year, month);
  return year > 0 && day > 0 && day <= maximumDay;
}

/** @param {number} year @param {number} month */
function maximumDayOfMonth(year, month) {
  if ([1, 3, 5, 7, 8, 10, 12].includes(month)) {
    return 31;
  }
  if ([4, 6, 9, 11].includes(month)) {
    return 30;
  }
  if (month !== 2) {
    return 0;
  }

  const isLeapYear = year % 4 === 0 &&
    (year % 100 !== 0 || year % 400 === 0);
  return isLeapYear ? 29 : 28;
}

/** @param {Date} now @param {string} timezone */
function currentForecastKey(now, timezone) {
  return forecastTime(now, timezone).key;
}

/** @param {Date} instant @param {string} timezone */
function forecastTime(instant, timezone) {
  const formatter = new Intl.DateTimeFormat("en-CA", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    hourCycle: "h23",
  });
  const parts = Object.fromEntries(
    formatter.formatToParts(instant).map(({ type, value }) => [type, value]),
  );
  return {
    key: `${parts.year}-${parts.month}-${parts.day}:${parts.hour}`,
    hour: Number(parts.hour),
  };
}

/** @param {string} date @param {number} hour */
function forecastKey(date, hour) {
  return `${date}:${String(hour).padStart(2, "0")}`;
}

/**
 * @param {CurrentForecast | null} left
 * @param {CurrentForecast | null} right
 */
function currentForecastsAreEqual(left, right) {
  return left === right ||
    left !== null &&
      right !== null &&
      left.celsius === right.celsius &&
      left.state === right.state &&
      left.description === right.description;
}
