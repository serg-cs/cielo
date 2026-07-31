import {
  fetchValidatedJson,
  readValidatedJson,
} from "./weather-data-client.js";

const hourlyForecastPeriodCount = 24;
const dailyForecastPeriodCount = 7;
const refreshIntervalMilliseconds = 30 * 60 * 1_000;
const hourMilliseconds = 60 * 60 * 1_000;
const minuteMilliseconds = 60 * 1_000;
const temperatureMinimum = -32_768;
const temperatureMaximum = 32_767;
const forecastBundleRangeSize = 20;
const forecastSchemaVersion = 2;
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
 * @typedef {object} DailySummary
 * @property {number} minimumTemperatureCelsius
 * @property {number} maximumTemperatureCelsius
 *
 * @property {string | null} condition
 * @property {string | null} description
 */

/**
 * @typedef {object} DailyForecastPeriod
 * @property {string} date
 * @property {boolean} isToday
 * @property {number | null} minimumTemperatureCelsius
 * @property {number | null} maximumTemperatureCelsius
 *
 * @property {string | null} condition
 * @property {string | null} description
 */

/**
 * @typedef {object} ParsedForecast
 * @property {string} municipalityId
 * @property {Map<string, CurrentConditions>} forecastsByHour
 * @property {Map<string, DailySummary>} dailySummariesByDate
 * @property {Map<string, ForecastEvent[]>} eventsByDate
 */

/** @typedef {Map<string, ParsedForecast>} ForecastBundle */

/**
 * @typedef {object} ForecastEvent
 * @property {"sunrise" | "sunset"} kind
 * @property {string} time
 */

/**
 * @typedef {ParsedForecast & {timeZone: string}} ForecastTimeline
 */

/**
 * @typedef {object} HourlyForecastPeriod
 * @property {"forecast"} kind
 * @property {string} date
 * @property {number} hour
 * @property {CurrentConditions | null} forecast
 */

/**
 * @typedef {object} SolarEventPeriod
 * @property {"sunrise" | "sunset"} kind
 * @property {string} time
 */

/** @typedef {HourlyForecastPeriod | SolarEventPeriod} HourlyTimelinePeriod */

/**
 * @typedef {object} CurrentConditionsChangeDetail
 * @property {string} municipalityId
 * @property {CurrentConditions | null} currentConditions
 */

/**
 * @param {unknown} document
 * @returns {ForecastBundle}
 */
export function validateForecastBundle(document) {
  if (
    typeof document !== "object" ||
    document === null ||
    !("schema_version" in document) ||
    document.schema_version !== forecastSchemaVersion ||
    !("forecasts" in document) ||
    typeof document.forecasts !== "object" ||
    document.forecasts === null ||
    Array.isArray(document.forecasts)
  ) {
    throw new Error("El documento de previsiones no es válido");
  }

  // Validate every keyed member before exposing any forecast from the bundle.
  const forecasts = new Map();
  for (const [municipalityId, forecastDocument] of Object.entries(
    document.forecasts,
  )) {
    if (!isMunicipalityId(municipalityId)) {
      throw new Error("El documento de previsiones no es válido");
    }
    const forecast = validateForecastDocument(
      forecastDocument,
      municipalityId,
    );
    forecasts.set(municipalityId, forecast);
  }
  if (forecasts.size === 0) {
    throw new Error("El documento de previsiones no es válido");
  }
  return forecasts;
}

/**
 * @param {unknown} document
 * @param {string} municipalityId
 * @returns {ParsedForecast}
 */
function validateForecastDocument(document, municipalityId) {
  if (
    !Array.isArray(document) ||
    document.length === 0
  ) {
    throw new Error("El documento de previsión no es válido");
  }

  // Index exact local hours while rejecting ambiguous forecast documents.
  const forecastsByHour = new Map();
  const dailySummariesByDate = new Map();
  const eventsByDate = new Map();
  const forecastDates = new Set();
  for (const day of document) {
    if (!isForecastDay(day) || forecastDates.has(day.date)) {
      throw new Error("El documento de previsión no es válido");
    }
    forecastDates.add(day.date);
    dailySummariesByDate.set(day.date, {
      minimumTemperatureCelsius: day.summary.temp_min_c,
      maximumTemperatureCelsius: day.summary.temp_max_c,
      condition: day.summary.state,
      description: day.summary.desc,
    });
    eventsByDate.set(day.date, day.events);

    for (const hourlyForecast of day.hours) {
      const key = forecastKey(day.date, hourlyForecast.hour);
      if (forecastsByHour.has(key)) {
        throw new Error("El documento de previsión contiene horas duplicadas");
      }
      forecastsByHour.set(key, {
        temperatureCelsius: hourlyForecast.temp_c,
        condition: hourlyForecast.state,
        description: hourlyForecast.desc,
      });
    }
  }
  if (forecastsByHour.size === 0) {
    throw new Error("El documento de previsión no contiene horas");
  }

  return {
    municipalityId,
    forecastsByHour,
    dailySummariesByDate,
    eventsByDate,
  };
}

/**
 * Select only a catalog-compatible member without coupling unrelated entries.
 *
 * @param {ForecastBundle} bundle
 * @param {Municipality} municipality
 * @returns {ForecastTimeline | null}
 */
function forecastForMunicipality(bundle, municipality) {
  const forecast = bundle.get(municipality.id);
  return forecast === undefined
    ? null
    : {
      ...forecast,
      timeZone: municipality.timeZone,
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
 * @returns {DailySummary | null}
 */
export function selectCurrentDaySummary(forecast, now = new Date()) {
  const { date } = forecastTime(now, forecast.timeZone);
  return forecast.dailySummariesByDate.get(date) ?? null;
}

/**
 * @param {ForecastTimeline} forecast
 * @param {Date} [now]
 * @returns {DailyForecastPeriod[]}
 */
export function selectDailyForecastPeriods(forecast, now = new Date()) {
  return dailyForecastPeriodsFor(
    forecast.timeZone,
    forecast.dailySummariesByDate,
    now,
  );
}

function dailyForecastPeriodsFor(timeZone, dailySummariesByDate, now) {
  const { date: currentDate } = forecastTime(now, timeZone);

  // Preserve interior gaps while removing unsupported dates from the end.
  const candidates = Array.from(
    { length: dailyForecastPeriodCount },
    (_, index) => {
      const date = addForecastDays(currentDate, index);
      return {
        date,
        index,
        summary: dailySummariesByDate.get(date),
      };
    },
  );
  while (
    candidates.length > 0 &&
    candidates[candidates.length - 1]?.summary === undefined
  ) {
    candidates.pop();
  }

  return candidates.map(({ date, index, summary }) => {
    return {
      date,
      isToday: index === 0,
      minimumTemperatureCelsius:
        summary?.minimumTemperatureCelsius ?? null,
      maximumTemperatureCelsius:
        summary?.maximumTemperatureCelsius ?? null,
      condition: summary?.condition ?? null,
      description: summary?.description ?? null,
    };
  });
}

/**
 * @param {ForecastTimeline} forecast
 * @param {Date} [now]
 * @returns {HourlyTimelinePeriod[]}
 */
export function selectHourlyForecastPeriods(forecast, now = new Date()) {
  const currentHour = now.getTime() - now.getTime() % hourMilliseconds;
  const currentTime = forecastTime(now, forecast.timeZone);

  // Walk real elapsed hours so local labels remain correct across day and DST changes.
  const hourlyPeriods = Array.from(
    { length: hourlyForecastPeriodCount },
    (_, index) => {
      const instant = new Date(currentHour + index * hourMilliseconds);
      const { date, key, hour } = forecastTime(instant, forecast.timeZone);
      return {
        kind: "forecast",
        date,
        hour,
        forecast: forecast.forecastsByHour.get(key) ?? null,
      };
    },
  );

  // Insert each event once after the local hour containing it.
  const timeline = [];
  const insertedEvents = new Set();
  for (const [index, period] of hourlyPeriods.entries()) {
    timeline.push(period);
    const events = forecast.eventsByDate.get(period.date);
    if (events === undefined) {
      continue;
    }

    for (const event of events) {
      const eventKey = `${period.date}:${event.kind}`;
      const { hour, minute } = solarTimeParts(event.time);
      const occurredThisHour = index === 0 &&
        hour === currentTime.hour &&
        minute <= currentTime.minute;
      if (
        hour === period.hour &&
        !occurredThisHour &&
        !insertedEvents.has(eventKey)
      ) {
        timeline.push(event);
        insertedEvents.add(eventKey);
      }
    }
  }
  return timeline;
}

/**
 * @param {CurrentConditions | null} currentConditions
 * @param {HourlyTimelinePeriod[]} hourlyForecastPeriods
 * @param {DailyForecastPeriod[]} dailyForecastPeriods
 * @returns {boolean}
 */
function forecastPeriodsAreUsable(
  currentConditions,
  hourlyForecastPeriods,
  dailyForecastPeriods,
) {
  return currentConditions !== null ||
    hourlyForecastPeriods.some(
      (period) => period.kind === "forecast" && period.forecast !== null,
    ) ||
    dailyForecastPeriods.some(
      (period) => period.minimumTemperatureCelsius !== null,
    );
}

export class ForecastStore extends EventTarget {
  #catalogById = new Map();
  #savedMunicipalityIds = new Set();
  #forecasts = new Map();
  #currentConditionsById = new Map();
  #dailySummariesById = new Map();
  #dailyForecastPeriodsById = new Map();
  #hourlyForecastPeriodsById = new Map();
  #forecastStatuses = new Map();
  #cacheReads = new Map();
  #inFlight = new Map();
  #weatherDataUrl;
  #fetcher;
  #now;
  #running = false;
  #refreshTimeoutId = null;
  #hourTimeoutId = null;
  #minuteTimeoutId = null;

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
    this.#scheduleMinuteBoundary();
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
        this.#publishDailySummary(id, null);
        this.#publishDailyForecastPeriods(id, []);
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

  /** @param {string} municipalityId @returns {DailySummary | null} */
  getCurrentDaySummary(municipalityId) {
    return this.#dailySummariesById.get(municipalityId) ?? null;
  }

  /** @param {string} municipalityId @returns {DailyForecastPeriod[]} */
  getDailyForecastPeriods(municipalityId) {
    const forecast = this.#forecasts.get(municipalityId);
    if (forecast !== undefined) {
      return selectDailyForecastPeriods(forecast, this.#now());
    }

    const municipality = this.#catalogById.get(municipalityId);
    return municipality === undefined
      ? []
      : dailyForecastPeriodsFor(
        municipality.timeZone,
        new Map(),
        this.#now(),
      );
  }

  /** @param {string} municipalityId @returns {HourlyTimelinePeriod[]} */
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
      const bundle = await this.#readForecastBundle(municipality.id);
      const forecast = bundle === null
        ? null
        : forecastForMunicipality(bundle, municipality);
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

    try {
      await this.#loadForecast(municipality);
    } catch (error) {
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
    }
  }

  /** @param {Municipality} municipality */
  async #loadForecast(municipality) {
    const bundle = await this.#fetchForecastBundle(municipality.id);
    if (!this.#savedMunicipalityIds.has(municipality.id)) {
      return;
    }

    const forecast = forecastForMunicipality(bundle, municipality);
    if (forecast === null) {
      throw new Error("El lote no contiene la previsión solicitada");
    }
    if (!this.#storeForecast(municipality.id, forecast)) {
      throw new Error("La previsión no contiene datos vigentes");
    }
  }

  /**
   * Share one Cache Storage read between municipalities in the same bundle.
   *
   * @param {string} municipalityId
   * @returns {Promise<ForecastBundle | null>}
   */
  async #readForecastBundle(municipalityId) {
    const url = forecastBundleUrl(this.#weatherDataUrl, municipalityId);
    const key = url.href;
    const existingRequest = this.#cacheReads.get(key);
    if (existingRequest !== undefined) {
      return existingRequest;
    }

    const request = readValidatedJson(
      url,
      validateForecastBundle,
    ).finally(() => {
      if (this.#cacheReads.get(key) === request) {
        this.#cacheReads.delete(key);
      }
    });
    this.#cacheReads.set(key, request);
    return request;
  }

  /**
   * Share one network refresh between municipalities in the same bundle.
   *
   * @param {string} municipalityId
   * @returns {Promise<ForecastBundle>}
   */
  async #fetchForecastBundle(municipalityId) {
    const url = forecastBundleUrl(this.#weatherDataUrl, municipalityId);
    const key = url.href;
    const existingRequest = this.#inFlight.get(key);
    if (existingRequest !== undefined) {
      return existingRequest;
    }

    const request = fetchValidatedJson(
      url,
      validateForecastBundle,
      this.#fetcher,
    ).finally(() => {
      if (this.#inFlight.get(key) === request) {
        this.#inFlight.delete(key);
      }
    });
    this.#inFlight.set(key, request);
    return request;
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
    const dailySummary = selectCurrentDaySummary(forecast, now);
    const dailyForecastPeriods = selectDailyForecastPeriods(forecast, now);
    const hourlyForecastPeriods = selectHourlyForecastPeriods(forecast, now);
    this.#publishCurrentConditions(
      municipalityId,
      currentConditions,
    );
    this.#publishDailySummary(municipalityId, dailySummary);
    this.#publishDailyForecastPeriods(
      municipalityId,
      dailyForecastPeriods,
    );
    this.#publishHourlyForecastPeriods(
      municipalityId,
      hourlyForecastPeriods,
    );

    // A valid document is ready only while it covers a visible forecast period.
    const usable = forecastPeriodsAreUsable(
      currentConditions,
      hourlyForecastPeriods,
      dailyForecastPeriods,
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
      if (forecast === undefined) {
        const municipality = this.#catalogById.get(municipalityId);
        if (municipality !== undefined) {
          this.#publishDailyForecastPeriods(
            municipalityId,
            dailyForecastPeriodsFor(
              municipality.timeZone,
              new Map(),
              now,
            ),
          );
        }
        continue;
      }

      const currentConditions = selectCurrentConditions(forecast, now);
      const dailySummary = selectCurrentDaySummary(forecast, now);
      const dailyForecastPeriods = selectDailyForecastPeriods(forecast, now);
      const hourlyForecastPeriods = selectHourlyForecastPeriods(forecast, now);
      this.#publishCurrentConditions(
        municipalityId,
        currentConditions,
      );
      this.#publishDailySummary(municipalityId, dailySummary);
      this.#publishDailyForecastPeriods(
        municipalityId,
        dailyForecastPeriods,
      );
      this.#publishHourlyForecastPeriods(
        municipalityId,
        hourlyForecastPeriods,
      );

      // Reconcile status in both directions as the rolling window advances.
      if (
        forecastPeriodsAreUsable(
          currentConditions,
          hourlyForecastPeriods,
          dailyForecastPeriods,
        )
      ) {
        this.#publishForecastStatus(municipalityId, "ready");
      } else if (this.#forecastStatuses.get(municipalityId) === "ready") {
        this.#publishForecastStatus(
          municipalityId,
          navigator.onLine ? "loading" : "offline",
        );
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
    const dailyForecastPeriods = selectDailyForecastPeriods(forecast, now);
    return forecastPeriodsAreUsable(
      selectCurrentConditions(forecast, now),
      selectHourlyForecastPeriods(forecast, now),
      dailyForecastPeriods,
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

  /** @param {string} municipalityId @param {DailySummary | null} dailySummary */
  #publishDailySummary(municipalityId, dailySummary) {
    const hasPrevious = this.#dailySummariesById.has(municipalityId);
    const previous = this.#dailySummariesById.get(municipalityId) ?? null;
    if (
      (!hasPrevious && dailySummary === null) ||
      dailySummariesAreEqual(previous, dailySummary)
    ) {
      return;
    }

    if (dailySummary === null) {
      this.#dailySummariesById.delete(municipalityId);
    } else {
      this.#dailySummariesById.set(municipalityId, dailySummary);
    }
    this.dispatchEvent(
      new CustomEvent("dailysummarychange", {
        detail: { municipalityId, dailySummary },
      }),
    );
  }

  /**
   * @param {string} municipalityId
   * @param {DailyForecastPeriod[]} periods
   */
  #publishDailyForecastPeriods(municipalityId, periods) {
    const previous = this.#dailyForecastPeriodsById.get(municipalityId);
    if (
      (previous === undefined && periods.length === 0) ||
      previous !== undefined &&
        dailyForecastPeriodsAreEqual(previous, periods)
    ) {
      return;
    }

    if (periods.length === 0) {
      this.#dailyForecastPeriodsById.delete(municipalityId);
    } else {
      this.#dailyForecastPeriodsById.set(municipalityId, periods);
    }
    this.dispatchEvent(
      new CustomEvent("dailyforecastchange", {
        detail: { municipalityId, periods },
      }),
    );
  }

  /**
   * @param {string} municipalityId
   * @param {HourlyTimelinePeriod[]} periods
   */
  #publishHourlyForecastPeriods(municipalityId, periods) {
    const previous = this.#hourlyForecastPeriodsById.get(municipalityId);
    if (
      (previous === undefined && periods.length === 0) ||
      previous !== undefined && hourlyForecastPeriodsAreEqual(previous, periods)
    ) {
      return;
    }

    if (periods.length === 0) {
      this.#hourlyForecastPeriodsById.delete(municipalityId);
    } else {
      this.#hourlyForecastPeriodsById.set(municipalityId, periods);
    }
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

  #scheduleMinuteBoundary() {
    if (this.#minuteTimeoutId !== null) {
      window.clearTimeout(this.#minuteTimeoutId);
      this.#minuteTimeoutId = null;
    }
    if (!this.#running || document.visibilityState !== "visible") {
      return;
    }

    const now = this.#now();
    const millisecondsUntilNextMinute =
      minuteMilliseconds - (now.getTime() % minuteMilliseconds);
    this.#minuteTimeoutId = window.setTimeout(() => {
      this.#minuteTimeoutId = null;
      this.#recomputeForecastSelections();
      this.#scheduleMinuteBoundary();
    }, millisecondsUntilNextMinute + 50);
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
    if (this.#minuteTimeoutId !== null) {
      window.clearTimeout(this.#minuteTimeoutId);
      this.#minuteTimeoutId = null;
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
      this.#scheduleMinuteBoundary();
      return;
    }

    this.#clearTimers();
  };
}

/** @param {URL} weatherDataUrl @param {string} municipalityId */
export function forecastBundleUrl(weatherDataUrl, municipalityId) {
  if (!isMunicipalityId(municipalityId)) {
    throw new Error("El identificador del municipio no es válido");
  }

  const province = municipalityId.slice(0, 2);
  const municipalityNumber = Number(municipalityId.slice(2));
  const rangeStart =
    Math.floor(municipalityNumber / forecastBundleRangeSize) *
    forecastBundleRangeSize;
  return new URL(
    `forecasts/${province}/${String(rangeStart).padStart(3, "0")}.json`,
    weatherDataUrl,
  );
}

/** @param {unknown} value @returns {value is string} */
function isMunicipalityId(value) {
  return typeof value === "string" && /^\d{5}$/u.test(value);
}

/** @param {unknown} value */
function isForecastDay(value) {
  if (
    typeof value === "object" &&
    value !== null &&
    "date" in value &&
    typeof value.date === "string" &&
    isForecastDate(value.date) &&
    "summary" in value &&
    isDailySummary(value.summary) &&
    "events" in value &&
    Array.isArray(value.events) &&
    value.events.every(isForecastEvent) &&
    "hours" in value &&
    Array.isArray(value.hours) &&
    value.hours.every(isForecastHour)
  ) {
    const eventKinds = new Set(value.events.map((event) => event.kind));
    if (eventKinds.size !== value.events.length) {
      return false;
    }
    const sunrise = value.events.find((event) => event.kind === "sunrise");
    const sunset = value.events.find((event) => event.kind === "sunset");
    return (
      (sunrise === undefined && sunset === undefined) ||
      (
        sunrise !== undefined &&
        sunset !== undefined &&
        sunrise.time < sunset.time
      )
    );
  }
  return false;
}

/** @param {unknown} value */
function isDailySummary(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    "temp_min_c" in value &&
    Number.isInteger(value.temp_min_c) &&
    value.temp_min_c >= temperatureMinimum &&
    value.temp_min_c <= temperatureMaximum &&
    "temp_max_c" in value &&
    Number.isInteger(value.temp_max_c) &&
    value.temp_max_c >= temperatureMinimum &&
    value.temp_max_c <= temperatureMaximum &&
    value.temp_min_c <= value.temp_max_c &&
    "state" in value &&
    "desc" in value &&
    (
      value.state === null && value.desc === null ||
      (
        typeof value.state === "string" &&
        supportedConditions.has(value.state) &&
        typeof value.desc === "string" &&
        value.desc.trim() !== ""
      )
    )
  );
}

/** @param {unknown} value */
function isForecastEvent(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    "kind" in value &&
    (value.kind === "sunrise" || value.kind === "sunset") &&
    "time" in value &&
    isSolarTime(value.time)
  );
}

/** @param {unknown} value */
function isForecastHour(value) {
  return (
    typeof value === "object" &&
    value !== null &&
    "hour" in value &&
    Number.isInteger(value.hour) &&
    value.hour >= 0 &&
    value.hour <= 23 &&
    "temp_c" in value &&
    Number.isInteger(value.temp_c) &&
    value.temp_c >= temperatureMinimum &&
    value.temp_c <= temperatureMaximum &&
    "state" in value &&
    typeof value.state === "string" &&
    supportedConditions.has(value.state) &&
    "desc" in value &&
    typeof value.desc === "string" &&
    value.desc.trim().length > 0
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

/** @param {unknown} value */
function isSolarTime(value) {
  if (typeof value !== "string") {
    return false;
  }
  const match = /^(\d{2}):(\d{2})$/u.exec(value);
  return match !== null &&
    Number(match[1]) <= 23 &&
    Number(match[2]) <= 59;
}

/** @param {string} time */
function solarTimeParts(time) {
  return {
    hour: Number(time.slice(0, 2)),
    minute: Number(time.slice(3, 5)),
  };
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
    minute: "2-digit",
    hourCycle: "h23",
  });
  const parts = Object.fromEntries(
    formatter.formatToParts(instant).map(({ type, value }) => [type, value]),
  );
  return {
    date: `${parts.year}-${parts.month}-${parts.day}`,
    key: `${parts.year}-${parts.month}-${parts.day}:${parts.hour}`,
    hour: Number(parts.hour),
    minute: Number(parts.minute),
  };
}

/** @param {string} date @param {number} dayOffset */
function addForecastDays(date, dayOffset) {
  const year = Number(date.slice(0, 4));
  const month = Number(date.slice(5, 7));
  const day = Number(date.slice(8, 10));
  const shifted = new Date(Date.UTC(year, month - 1, day + dayOffset));
  return [
    String(shifted.getUTCFullYear()).padStart(4, "0"),
    String(shifted.getUTCMonth() + 1).padStart(2, "0"),
    String(shifted.getUTCDate()).padStart(2, "0"),
  ].join("-");
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

function dailySummariesAreEqual(left, right) {
  return (
    left === right ||
    (
      left !== null &&
      right !== null &&
      left.minimumTemperatureCelsius === right.minimumTemperatureCelsius &&
      left.maximumTemperatureCelsius === right.maximumTemperatureCelsius &&
      left.condition === right.condition &&
      left.description === right.description
    )
  );
}

/**
 * @param {DailyForecastPeriod[]} left
 * @param {DailyForecastPeriod[]} right
 */
function dailyForecastPeriodsAreEqual(left, right) {
  return left.length === right.length &&
    left.every((period, index) => {
      const other = right[index];
      return other !== undefined &&
        period.date === other.date &&
        period.isToday === other.isToday &&
        period.minimumTemperatureCelsius ===
          other.minimumTemperatureCelsius &&
        period.maximumTemperatureCelsius ===
          other.maximumTemperatureCelsius &&
        period.condition === other.condition &&
        period.description === other.description;
    });
}

/**
 * @param {HourlyTimelinePeriod[]} left
 * @param {HourlyTimelinePeriod[]} right
 */
function hourlyForecastPeriodsAreEqual(left, right) {
  return left.length === right.length &&
    left.every((period, index) => {
      const other = right[index];
      if (other === undefined || period.kind !== other.kind) {
        return false;
      }
      if (period.kind !== "forecast" || other.kind !== "forecast") {
        return period.time === other.time;
      }
      return period.date === other.date &&
        period.hour === other.hour &&
        currentConditionsAreEqual(period.forecast, other.forecast);
    });
}
