import {
  fetchValidatedJson,
  readValidatedJson,
} from "./weather-data-client.js";

const generatorIdentity = "cielo";
const hourlyForecastPeriodCount = 24;
const refreshIntervalMilliseconds = 30 * 60 * 1_000;
const hourMilliseconds = 60 * 60 * 1_000;
const temperatureMinimum = -32_768;
const temperatureMaximum = 32_767;
const supportedConditions = new Set([
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

/** @typedef {import("./municipality-catalog.js").Municipality} Municipality */
/** @typedef {"loading" | "ready" | "offline" | "error"} ForecastStatus */

/**
 * @typedef {object} CurrentConditions
 * @property {number} temperatureCelsius
 * @property {string} condition
 * @property {string} description
 */

/**
 * @typedef {object} ForecastTimeline
 * @property {string} municipalityId
 * @property {string} timeZone
 * @property {Map<string, CurrentConditions>} forecastsByHour
 */

/**
 * @typedef {object} HourlyForecastPeriod
 * @property {number} hour
 * @property {CurrentConditions | null} forecast
 */

/**
 * @typedef {object} CurrentConditionsChangeDetail
 * @property {string} municipalityId
 * @property {CurrentConditions | null} currentConditions
 */

/**
 * @param {unknown} document
 * @param {Municipality} municipality
 * @returns {ForecastTimeline}
 */
export function validateForecastDocument(document, municipality) {
  if (
    typeof document !== "object" ||
    document === null ||
    !("generator" in document) ||
    document.generator !== generatorIdentity ||
    !("municipality_id" in document) ||
    document.municipality_id !== municipality.id ||
    !("time_zone" in document) ||
    document.time_zone !== municipality.timeZone ||
    !("hourly_forecasts" in document) ||
    !Array.isArray(document.hourly_forecasts) ||
    document.hourly_forecasts.length === 0
  ) {
    throw new Error("El documento de previsión no es válido");
  }

  // Index exact local hours while rejecting ambiguous forecast documents.
  const forecastsByHour = new Map();
  for (const hourlyForecast of document.hourly_forecasts) {
    if (!isHourlyForecast(hourlyForecast)) {
      throw new Error("El documento de previsión no es válido");
    }

    const key = forecastKey(hourlyForecast.date, hourlyForecast.hour);
    if (forecastsByHour.has(key)) {
      throw new Error("El documento de previsión contiene horas duplicadas");
    }
    forecastsByHour.set(key, {
      temperatureCelsius: hourlyForecast.temperature_celsius,
      condition: hourlyForecast.condition,
      description: hourlyForecast.description,
    });
  }

  return {
    municipalityId: municipality.id,
    timeZone: municipality.timeZone,
    forecastsByHour,
  };
}

/**
 * @param {ForecastTimeline} forecast
 * @param {Date} [now]
 * @returns {CurrentConditions | null}
 */
export function selectCurrentConditions(forecast, now = new Date()) {
  const key = currentConditionsKey(now, forecast.timeZone);
  return forecast.forecastsByHour.get(key) ?? null;
}

/**
 * @param {ForecastTimeline} forecast
 * @param {Date} [now]
 * @returns {HourlyForecastPeriod[]}
 */
export function selectHourlyForecastPeriods(forecast, now = new Date()) {
  const currentHour = now.getTime() - now.getTime() % hourMilliseconds;

  // Walk real elapsed hours so local labels remain correct across day and DST changes.
  return Array.from({ length: hourlyForecastPeriodCount }, (_, index) => {
    const instant = new Date(currentHour + index * hourMilliseconds);
    const { key, hour } = forecastTime(instant, forecast.timeZone);
    return {
      hour,
      forecast: forecast.forecastsByHour.get(key) ?? null,
    };
  });
}

/**
 * @param {CurrentConditions | null} currentConditions
 * @param {HourlyForecastPeriod[]} hourlyForecastPeriods
 * @returns {boolean}
 */
function forecastPeriodsAreUsable(currentConditions, hourlyForecastPeriods) {
  return currentConditions !== null ||
    hourlyForecastPeriods.some((period) => period.forecast !== null);
}

export class ForecastStore extends EventTarget {
  #catalogById = new Map();
  #savedMunicipalityIds = new Set();
  #forecasts = new Map();
  #currentConditionsById = new Map();
  #forecastStatuses = new Map();
  #inFlight = new Map();
  #weatherDataUrl;
  #fetcher;
  #now;
  #running = false;
  #refreshTimeoutId = null;
  #hourTimeoutId = null;

  /**
   * @param {URL} weatherDataUrl
   * @param {{fetcher?: typeof fetch, now?: () => Date}} [options]
   */
  constructor(
    weatherDataUrl,
    {
      fetcher = globalThis.fetch.bind(globalThis),
      now = () => new Date(),
    } = {},
  ) {
    super();
    this.#weatherDataUrl = weatherDataUrl;
    this.#fetcher = fetcher;
    this.#now = now;
  }

  /**
   * @param {Municipality[]} municipalities
   * @param {Set<string>} savedMunicipalityIds
   */
  start(municipalities, savedMunicipalityIds) {
    this.#catalogById = new Map(
      municipalities.map((municipality) => [municipality.id, municipality]),
    );
    this.setSavedMunicipalityIds(savedMunicipalityIds);
    if (!this.#running) {
      this.#running = true;
      window.addEventListener("online", this.#handleOnline);
      window.addEventListener("offline", this.#handleOffline);
      document.addEventListener("visibilitychange", this.#handleVisibilityChange);
    }

    // Recompute memory state, then hydrate from storage before using the network.
    this.#recomputeForecastSelections();
    void this.#hydrateAndRefreshIds([...this.#savedMunicipalityIds]);
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

  /** @param {Set<string>} savedMunicipalityIds */
  setSavedMunicipalityIds(savedMunicipalityIds) {
    const nextIds = new Set(
      [...savedMunicipalityIds].filter((id) => this.#catalogById.has(id)),
    );
    const addedIds = [...nextIds].filter(
      (id) => !this.#savedMunicipalityIds.has(id),
    );

    // Release in-memory data for locations that no longer participate in the UI.
    for (const id of this.#savedMunicipalityIds) {
      if (!nextIds.has(id)) {
        this.#forecasts.delete(id);
        this.#forecastStatuses.delete(id);
        this.#publishCurrentConditions(id, null);
        this.#publishHourlyForecastPeriods(id, []);
      }
    }
    this.#savedMunicipalityIds = nextIds;
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

  /** @param {string} municipalityId @returns {CurrentConditions | null} */
  getCurrentConditions(municipalityId) {
    return this.#currentConditionsById.get(municipalityId) ?? null;
  }

  /** @param {string} municipalityId @returns {HourlyForecastPeriod[]} */
  getHourlyForecastPeriods(municipalityId) {
    const forecast = this.#forecasts.get(municipalityId);
    return forecast === undefined
      ? []
      : selectHourlyForecastPeriods(forecast, this.#now());
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
    await this.#refreshIds([...this.#savedMunicipalityIds]);
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
    if (
      municipality === undefined ||
      !this.#savedMunicipalityIds.has(municipalityId)
    ) {
      return;
    }

    if (!this.#forecasts.has(municipalityId)) {
      const forecast = await readValidatedJson(
        forecastUrl(this.#weatherDataUrl, municipality.id),
        (document) => validateForecastDocument(document, municipality),
      );
      if (
        forecast !== null &&
        this.#savedMunicipalityIds.has(municipalityId) &&
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
    if (
      municipality === undefined ||
      !this.#savedMunicipalityIds.has(municipalityId)
    ) {
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
          `No se pudo actualizar la previsión de ${municipality.name}`,
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
      forecastUrl(this.#weatherDataUrl, municipality.id),
      (document) => validateForecastDocument(document, municipality),
      this.#fetcher,
    );
    if (!this.#savedMunicipalityIds.has(municipality.id)) {
      return;
    }

    if (!this.#storeForecast(municipality.id, forecast)) {
      throw new Error("La previsión no contiene horas vigentes");
    }
  }

  /**
   * @param {string} municipalityId
   * @param {ForecastTimeline} forecast
   * @returns {boolean}
   */
  #storeForecast(municipalityId, forecast) {
    this.#forecasts.set(municipalityId, forecast);
    const now = this.#now();
    const currentConditions = selectCurrentConditions(forecast, now);
    const hourlyForecastPeriods = selectHourlyForecastPeriods(forecast, now);
    this.#publishCurrentConditions(
      municipalityId,
      currentConditions,
    );
    this.#publishHourlyForecastPeriods(
      municipalityId,
      hourlyForecastPeriods,
    );

    // A valid document is ready only while it covers a visible forecast period.
    const usable = forecastPeriodsAreUsable(
      currentConditions,
      hourlyForecastPeriods,
    );
    if (usable) {
      this.#publishForecastStatus(municipalityId, "ready");
    }
    return usable;
  }

  #recomputeForecastSelections() {
    const now = this.#now();
    for (const municipalityId of this.#savedMunicipalityIds) {
      const forecast = this.#forecasts.get(municipalityId);
      if (forecast !== undefined) {
        const currentConditions = selectCurrentConditions(forecast, now);
        const hourlyForecastPeriods = selectHourlyForecastPeriods(forecast, now);
        this.#publishCurrentConditions(
          municipalityId,
          currentConditions,
        );
        this.#publishHourlyForecastPeriods(
          municipalityId,
          hourlyForecastPeriods,
        );

        // Reconcile status in both directions as the rolling window advances.
        if (forecastPeriodsAreUsable(currentConditions, hourlyForecastPeriods)) {
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
    return forecastPeriodsAreUsable(
      selectCurrentConditions(forecast, now),
      selectHourlyForecastPeriods(forecast, now),
    );
  }

  /** @param {string} municipalityId @param {CurrentConditions | null} currentConditions */
  #publishCurrentConditions(municipalityId, currentConditions) {
    const hasPrevious = this.#currentConditionsById.has(municipalityId);
    const previous = this.#currentConditionsById.get(municipalityId) ?? null;
    if (
      (!hasPrevious && currentConditions === null) ||
      currentConditionsAreEqual(previous, currentConditions)
    ) {
      return;
    }

    if (currentConditions === null) {
      this.#currentConditionsById.delete(municipalityId);
    } else {
      this.#currentConditionsById.set(municipalityId, currentConditions);
    }
    this.dispatchEvent(
      new CustomEvent("currentconditionschange", {
        detail: { municipalityId, currentConditions },
      }),
    );
  }

  /**
   * @param {string} municipalityId
   * @param {HourlyForecastPeriod[]} periods
   */
  #publishHourlyForecastPeriods(municipalityId, periods) {
    this.dispatchEvent(
      new CustomEvent("hourlyforecastchange", {
        detail: { municipalityId, periods },
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
    for (const municipalityId of this.#savedMunicipalityIds) {
      if (!this.#hasUsableForecast(municipalityId)) {
        this.#publishForecastStatus(municipalityId, "loading");
      }
    }
    this.#recomputeForecastSelections();
    void this.refreshNow();
  };

  #handleOffline = () => {
    for (const municipalityId of this.#savedMunicipalityIds) {
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

/** @param {URL} weatherDataUrl @param {string} municipalityId */
function forecastUrl(weatherDataUrl, municipalityId) {
  return new URL(
    `hourly_forecasts/${encodeURIComponent(municipalityId)}.json`,
    weatherDataUrl,
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
    "temperature_celsius" in value &&
    Number.isInteger(value.temperature_celsius) &&
    value.temperature_celsius >= temperatureMinimum &&
    value.temperature_celsius <= temperatureMaximum &&
    "condition" in value &&
    typeof value.condition === "string" &&
    supportedConditions.has(value.condition) &&
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

/** @param {Date} now @param {string} timeZone */
function currentConditionsKey(now, timeZone) {
  return forecastTime(now, timeZone).key;
}

/** @param {Date} instant @param {string} timeZone */
function forecastTime(instant, timeZone) {
  const formatter = new Intl.DateTimeFormat("en-CA", {
    timeZone,
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
 * @param {CurrentConditions | null} left
 * @param {CurrentConditions | null} right
 */
function currentConditionsAreEqual(left, right) {
  return left === right ||
    left !== null &&
      right !== null &&
      left.temperatureCelsius === right.temperatureCelsius &&
      left.condition === right.condition &&
      left.description === right.description;
}
