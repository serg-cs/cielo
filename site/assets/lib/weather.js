const forecastSchemaVersion = 1;
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

export class CurrentForecastStore extends EventTarget {
  #catalogById = new Map();
  #trackedIds = new Set();
  #forecasts = new Map();
  #currentForecasts = new Map();
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
      document.addEventListener("visibilitychange", this.#handleVisibilityChange);
      navigator.serviceWorker?.addEventListener(
        "controllerchange",
        this.#handleControllerChange,
      );
    }

    // Recompute immediately before starting the recurring refresh cycles.
    this.#recomputeCurrentForecasts();
    void this.refreshNow();
    this.#scheduleRefresh();
    this.#scheduleHourBoundary();
  }

  stop() {
    if (!this.#running) {
      return;
    }

    this.#running = false;
    window.removeEventListener("online", this.#handleOnline);
    document.removeEventListener(
      "visibilitychange",
      this.#handleVisibilityChange,
    );
    navigator.serviceWorker?.removeEventListener(
      "controllerchange",
      this.#handleControllerChange,
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
        this.#publishCurrentForecast(id, null);
      }
    }
    this.#trackedIds = nextIds;
    this.#recomputeCurrentForecasts();

    if (this.#running && addedIds.length > 0) {
      void this.#refreshIds(addedIds);
    }
  }

  /** @param {string} municipalityId @returns {CurrentForecast | null} */
  getCurrentForecast(municipalityId) {
    return this.#currentForecasts.get(municipalityId) ?? null;
  }

  async refreshNow() {
    await this.#refreshIds([...this.#trackedIds]);
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

    const request = this.#loadForecast(municipality)
      .catch((error) => {
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
    const url = new URL(
      `./data/temperatures/${encodeURIComponent(municipality.id)}.json`,
      document.baseURI,
    );
    const response = await this.#fetcher(url, { cache: "no-cache" });
    if (!response.ok) {
      throw new Error(
        `No se pudieron cargar las temperaturas: HTTP ${response.status}`,
      );
    }

    const forecast = validateForecastDocument(
      await response.json(),
      municipality,
    );
    if (!this.#trackedIds.has(municipality.id)) {
      return;
    }

    this.#forecasts.set(municipality.id, forecast);
    this.#publishCurrentForecast(
      municipality.id,
      selectCurrentForecast(forecast, this.#now()),
    );
  }

  #recomputeCurrentForecasts() {
    for (const municipalityId of this.#trackedIds) {
      const forecast = this.#forecasts.get(municipalityId);
      if (forecast !== undefined) {
        this.#publishCurrentForecast(
          municipalityId,
          selectCurrentForecast(forecast, this.#now()),
        );
      }
    }
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
      this.#recomputeCurrentForecasts();
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
    this.#recomputeCurrentForecasts();
    void this.refreshNow();
  };

  #handleVisibilityChange = () => {
    if (document.visibilityState === "visible") {
      this.#recomputeCurrentForecasts();
      void this.refreshNow();
      this.#scheduleRefresh();
      this.#scheduleHourBoundary();
      return;
    }

    this.#clearTimers();
  };

  #handleControllerChange = async () => {
    // Let uncontrolled requests settle before refetching through the new controller.
    const requestsBeforeTakeover = [...this.#inFlight.values()];
    await Promise.all(requestsBeforeTakeover);
    if (this.#running) {
      await this.refreshNow();
    }
  };
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
  const formatter = new Intl.DateTimeFormat("en-CA", {
    timeZone: timezone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    hourCycle: "h23",
  });
  const parts = Object.fromEntries(
    formatter.formatToParts(now).map(({ type, value }) => [type, value]),
  );
  return `${parts.year}-${parts.month}-${parts.day}:${parts.hour}`;
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
