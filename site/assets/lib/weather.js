const forecastSchemaVersion = 1;
const refreshIntervalMilliseconds = 30 * 60 * 1_000;
const hourMilliseconds = 60 * 60 * 1_000;
const temperatureMinimum = -32_768;
const temperatureMaximum = 32_767;

/** @typedef {import("./catalog.js").Municipality} Municipality */

/**
 * @typedef {object} TemperatureForecast
 * @property {string} municipalityId
 * @property {string} timezone
 * @property {Map<string, number>} temperaturesByHour
 */

/**
 * @typedef {object} TemperatureChangeDetail
 * @property {string} municipalityId
 * @property {number | null} celsius
 */

/**
 * @param {unknown} document
 * @param {Municipality} municipality
 * @returns {TemperatureForecast}
 */
export function validateTemperatureDocument(document, municipality) {
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
  const temperaturesByHour = new Map();
  for (const temperature of document.temperatures) {
    if (!isTemperature(temperature)) {
      throw new Error("El documento de temperaturas no es válido");
    }

    const key = temperatureKey(temperature.date, temperature.hour);
    if (temperaturesByHour.has(key)) {
      throw new Error("El documento de temperaturas contiene horas duplicadas");
    }
    temperaturesByHour.set(key, temperature.celsius);
  }

  return {
    municipalityId: municipality.id,
    timezone: municipality.timezone,
    temperaturesByHour,
  };
}

/**
 * @param {TemperatureForecast} forecast
 * @param {Date} [now]
 * @returns {number | null}
 */
export function selectCurrentTemperature(forecast, now = new Date()) {
  const key = currentTemperatureKey(now, forecast.timezone);
  return forecast.temperaturesByHour.get(key) ?? null;
}

export class CurrentTemperatureStore extends EventTarget {
  #catalogById = new Map();
  #trackedIds = new Set();
  #forecasts = new Map();
  #temperatures = new Map();
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
    this.#recomputeTemperatures();
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
        this.#publishTemperature(id, null);
      }
    }
    this.#trackedIds = nextIds;
    this.#recomputeTemperatures();

    if (this.#running && addedIds.length > 0) {
      void this.#refreshIds(addedIds);
    }
  }

  /** @param {string} municipalityId @returns {number | null} */
  getCurrentTemperature(municipalityId) {
    return this.#temperatures.get(municipalityId) ?? null;
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

    const forecast = validateTemperatureDocument(
      await response.json(),
      municipality,
    );
    if (!this.#trackedIds.has(municipality.id)) {
      return;
    }

    this.#forecasts.set(municipality.id, forecast);
    this.#publishTemperature(
      municipality.id,
      selectCurrentTemperature(forecast, this.#now()),
    );
  }

  #recomputeTemperatures() {
    for (const municipalityId of this.#trackedIds) {
      const forecast = this.#forecasts.get(municipalityId);
      if (forecast !== undefined) {
        this.#publishTemperature(
          municipalityId,
          selectCurrentTemperature(forecast, this.#now()),
        );
      }
    }
  }

  /** @param {string} municipalityId @param {number | null} celsius */
  #publishTemperature(municipalityId, celsius) {
    const hasPrevious = this.#temperatures.has(municipalityId);
    const previous = this.#temperatures.get(municipalityId) ?? null;
    if ((!hasPrevious && celsius === null) || previous === celsius) {
      return;
    }

    if (celsius === null) {
      this.#temperatures.delete(municipalityId);
    } else {
      this.#temperatures.set(municipalityId, celsius);
    }
    this.dispatchEvent(
      new CustomEvent("temperaturechange", {
        detail: { municipalityId, celsius },
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
      this.#recomputeTemperatures();
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
    this.#recomputeTemperatures();
    void this.refreshNow();
  };

  #handleVisibilityChange = () => {
    if (document.visibilityState === "visible") {
      this.#recomputeTemperatures();
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
function isTemperature(value) {
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
    value.celsius <= temperatureMaximum
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
function currentTemperatureKey(now, timezone) {
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
function temperatureKey(date, hour) {
  return `${date}:${String(hour).padStart(2, "0")}`;
}
