/**
 * Decoding of published forecasts and selectors used by the views.
 */

import { amount, degrees, localDate, localHour, relativeHour } from "./format.js";

const HOUR_MS = 3600 * 1000;

/** Day parts in display order. */
const DAY_PARTS = [
  { period: "00-06", label: "Madrugada" },
  { period: "06-12", label: "Mañana" },
  { period: "12-18", label: "Tarde" },
  { period: "18-24", label: "Noche" },
];
const HALF_DAYS = [
  { period: "00-12", label: "Mañana" },
  { period: "12-24", label: "Tarde y noche" },
];

/**
 * @typedef {object} Hour
 * @property {number} time Start of the hour (ms since epoch).
 * @property {string | null} sky AEMET sky code.
 * @property {number | null} temp
 * @property {number | null} feels
 * @property {number | string | null} rain mm, or `Ip` for a trace.
 * @property {number | string | null} snow
 * @property {number | null} rh
 * @property {string | null} wdir
 * @property {number | null} wind
 * @property {number | null} gust
 * @property {number | null} pop Probability of precipitation in this hour's window.
 * @property {number | null} storm
 * @property {number | null} snowProb
 */

/**
 * @typedef {object} Period
 * @property {string} [sky]
 * @property {number} [pop]
 * @property {number} [snowl]
 * @property {string} [wdir]
 * @property {number} [wind]
 * @property {number} [gust]
 */

/**
 * @typedef {object} Day
 * @property {string} date
 * @property {number | null} tmin
 * @property {number | null} tmax
 * @property {[number | null, number | null] | null} feels
 * @property {[number | null, number | null] | null} rh
 * @property {number | null} uv
 * @property {Record<string, number>} temps
 * @property {number | null} fire
 * @property {Record<string, Period>} periods
 * @property {[string | null, string | null] | null} sun Local sunrise and sunset.
 */

/**
 * @typedef {object} Forecast
 * @property {string} id
 * @property {string} timeZone
 * @property {{zone?: string, alt?: number}} meta Warning zone and altitude.
 * @property {Hour[]} hours
 * @property {Window[]} windows
 * @property {Day[]} days
 */

/**
 * @typedef {object} Window Probabilities for a span of hours.
 * @property {number} start
 * @property {number} end
 * @property {number | null} pop
 * @property {number | null} storm
 * @property {number | null} snow
 */

/**
 * Decode one municipality from a forecast bundle.
 *
 * @param {string} id
 * @param {any} raw
 * @param {string} timeZone
 * @returns {Forecast | null}
 */
export function decodeForecast(id, raw, timeZone) {
  if (!raw || typeof raw !== "object") return null;
  const windows = (Array.isArray(raw.windows) ? raw.windows : [])
    .map((window) => ({
      start: Date.parse(window.t),
      end: Date.parse(window.t) + Number(window.h) * HOUR_MS,
      pop: window.pop ?? null,
      storm: window.storm ?? null,
      snow: window.snow ?? null,
    }))
    .filter((window) => Number.isFinite(window.start) && window.end > window.start);

  /** @type {Hour[]} */
  const hours = [];
  const hourly = raw.hourly;
  const start = hourly ? Date.parse(hourly.t0) : Number.NaN;
  if (Number.isFinite(start)) {
    const count = Math.max(...["sky", "temp"].map((key) => (Array.isArray(hourly[key]) ? hourly[key].length : 0)));
    /** @param {string} key @param {number} index */
    const value = (key, index) => (Array.isArray(hourly[key]) ? (hourly[key][index] ?? null) : null);
    for (let index = 0; index < count; index += 1) {
      const time = start + index * HOUR_MS;
      const window = windows.find((candidate) => candidate.start <= time && time < candidate.end);
      hours.push({
        time,
        sky: value("sky", index),
        temp: value("temp", index),
        feels: value("feels", index),
        rain: value("rain", index),
        snow: value("snow", index),
        rh: value("rh", index),
        wdir: value("wdir", index),
        wind: value("wind", index),
        gust: value("gust", index),
        pop: window?.pop ?? null,
        storm: window?.storm ?? null,
        snowProb: window?.snow ?? null,
      });
    }
    fillSky(hours);
  }

  const sun = raw.sun && typeof raw.sun === "object" ? raw.sun : {};
  /** @type {Day[]} */
  const days = (Array.isArray(raw.daily) ? raw.daily : []).map((day) => ({
    date: day.date,
    tmin: day.tmin ?? null,
    tmax: day.tmax ?? null,
    feels: day.feels ?? null,
    rh: day.rh ?? null,
    uv: day.uv ?? null,
    temps: day.temps ?? {},
    fire: day.fire ?? null,
    periods: day.p ?? {},
    sun: sun[day.date] ?? null,
  }));

  return {
    id,
    timeZone,
    meta: { ...(raw.meta ?? {}) },
    hours,
    windows,
    days,
  };
}

/**
 * Hours with a temperature but no sky code take the nearest earlier code,
 * or the next one at the start.
 *
 * @param {Hour[]} hours
 */
function fillSky(hours) {
  let previous = null;
  for (const hour of hours) {
    if (hour.sky) previous = hour.sky;
    else if (previous && hour.temp !== null) hour.sky = previous;
  }
  let next = null;
  for (let index = hours.length - 1; index >= 0; index -= 1) {
    if (hours[index].sky) next = hours[index].sky;
    else if (next && hours[index].temp !== null) hours[index].sky = next;
  }
}

/**
 * The hour containing `now`.
 *
 * @param {Forecast} forecast
 * @param {number} now
 */
export function currentHour(forecast, now) {
  return forecast.hours.find((hour) => hour.time <= now && now < hour.time + HOUR_MS && hour.temp !== null) ?? null;
}

/**
 * Hours from the current one onwards that have data.
 *
 * @param {Forecast} forecast
 * @param {number} now
 * @param {number} [limit]
 */
export function upcomingHours(forecast, now, limit = 48) {
  return forecast.hours
    .filter((hour) => hour.time + HOUR_MS > now && (hour.temp !== null || hour.sky !== null))
    .slice(0, limit);
}

/**
 * Forecast days from local today, at most `limit`.
 *
 * @param {Forecast} forecast
 * @param {number} now
 * @param {number} [limit]
 */
export function upcomingDays(forecast, now, limit = 7) {
  const today = localDate(now, forecast.timeZone);
  return forecast.days.filter((day) => day.date >= today).slice(0, limit);
}

/**
 * The best whole-day or remaining-day summary values of a day.
 *
 * For today, periods that already ended are ignored so the icon and rain
 * probability describe what is still to come.
 *
 * @param {Day} day
 * @param {{today: string, hour: number}} context
 * @returns {{sky: string | null, pop: number | null, period: string | null, snowl: number | null, wind: number | null, gust: number | null, wdir: string | null}}
 */
export function daySummary(day, { today, hour }) {
  const order = ["00-24", "12-24", "00-12", "18-24", "12-18", "06-12", "00-06"];
  const isToday = day.date === today;
  const remaining = order.filter((period) => {
    if (!day.periods[period]) return false;
    return !isToday || Number(period.slice(3)) > hour;
  });
  // Today: the period from now to the end of the day that starts latest
  // (18-24 in the evening, 12-24 in the afternoon), else the widest left.
  const restOfDay = isToday
    ? remaining
        .filter((period) => period.endsWith("-24") && Number(period.slice(0, 2)) <= hour && day.periods[period].sky)
        .sort((left, right) => Number(right.slice(0, 2)) - Number(left.slice(0, 2)))[0]
    : undefined;
  const withSky = restOfDay ?? remaining.find((period) => day.periods[period].sky);
  const chosen = withSky ?? remaining[0] ?? null;
  const values = chosen ? day.periods[chosen] : {};
  const pops = remaining.map((period) => day.periods[period].pop).filter((pop) => typeof pop === "number");
  const snowLevels = remaining.map((period) => day.periods[period].snowl).filter((level) => typeof level === "number");
  const gusts = remaining.map((period) => day.periods[period].gust).filter((gust) => typeof gust === "number");
  return {
    sky: values.sky ?? null,
    pop: values.pop ?? (pops.length ? Math.max(...pops) : null),
    period: chosen,
    snowl: snowLevels.length ? Math.min(...snowLevels) : null,
    wind: values.wind ?? null,
    wdir: values.wdir ?? null,
    gust: gusts.length ? Math.max(...gusts) : null,
  };
}

/**
 * Day parts to show for a day: four parts on the first days, two halves
 * later, or nothing when only whole-day values exist.
 *
 * @param {Day} day
 */
export function dayParts(day) {
  const quarters = DAY_PARTS.filter(({ period }) => day.periods[period]);
  if (quarters.length >= 2)
    return quarters.map((part) => ({
      ...part,
      values: day.periods[part.period],
    }));
  const halves = HALF_DAYS.filter(({ period }) => day.periods[period]);
  if (halves.length >= 2)
    return halves.map((part) => ({
      ...part,
      values: day.periods[part.period],
    }));
  return [];
}

/**
 * Hours of one local date.
 *
 * @param {Forecast} forecast
 * @param {string} date
 */
export function hoursOfDay(forecast, date) {
  return forecast.hours.filter((hour) => localDate(hour.time, forecast.timeZone) === date);
}

/**
 * Highest storm and snow probabilities in the next `hours` hours.
 *
 * @param {Forecast} forecast
 * @param {number} now
 * @param {number} [span]
 */
export function upcomingRisks(forecast, now, span = 24) {
  const hours = upcomingHours(forecast, now, span);
  const maximum = (/** @type {(hour: Hour) => number | null} */ pick) =>
    hours.reduce((highest, hour) => Math.max(highest, pick(hour) ?? 0), 0);
  return {
    pop: maximum((hour) => hour.pop),
    storm: maximum((hour) => hour.storm),
    snow: maximum((hour) => hour.snowProb),
  };
}

/**
 * Short description of the coming hours: rain first, then temperature.
 *
 * @param {Hour[]} hours Upcoming hours, the first one being now.
 * @param {string} timeZone
 * @param {number} now
 * @returns {string[]} One or two sentences.
 */
export function hoursSummary(hours, timeZone, now) {
  if (hours.length === 0) return [];
  const today = localDate(now, timeZone);
  const wet = (/** @type {Hour} */ hour) => amount(hour.rain) >= 0.1 || amount(hour.snow) >= 0.1;
  const snowy = (/** @type {Hour} */ hour) => amount(hour.snow) >= 0.1;
  const sentences = [];
  const span = hours.length;
  const firstWet = hours.findIndex(wet);
  if (firstWet === 0) {
    const kind = snowy(hours[0]) ? "Nieve" : "Lluvia";
    const dry = hours.findIndex((hour) => !wet(hour));
    sentences.push(
      dry > 0
        ? `${kind} ahora; parará ${relativeHour(hours[dry].time, timeZone, today)}.`
        : `${kind} durante las próximas horas.`,
    );
  } else if (firstWet > 0) {
    const kind = snowy(hours[firstWet]) ? "Nieve" : "Lluvia";
    sentences.push(`${kind} prevista ${relativeHour(hours[firstWet].time, timeZone, today)}.`);
  } else {
    const likely = hours.find((hour) => (hour.pop ?? 0) >= 40);
    sentences.push(
      likely
        ? `Sin lluvia prevista, aunque es posible ${relativeHour(likely.time, timeZone, today)}.`
        : `Sin lluvia en las próximas ${span} horas.`,
    );
  }

  // Temperature: the next maximum during the day or minimum at night.
  const next24 = hours.slice(0, 24).filter((hour) => hour.temp !== null);
  if (next24.length > 2) {
    const hour = localHour(now, timeZone);
    const warmest = next24.reduce((best, item) =>
      /** @type {number} */ (item.temp) > /** @type {number} */ (best.temp) ? item : best,
    );
    const coldest = next24.reduce((best, item) =>
      /** @type {number} */ (item.temp) < /** @type {number} */ (best.temp) ? item : best,
    );
    const pick = hour >= 6 && hour < 15 ? { label: "Máxima", item: warmest } : { label: "Mínima", item: coldest };
    if (pick.item !== hours[0]) {
      sentences.push(`${pick.label} de ${degrees(pick.item.temp)} ${relativeHour(pick.item.time, timeZone, today)}.`);
    }
  }
  const gusty = hours.slice(0, 24).find((hour) => (hour.gust ?? 0) >= 50);
  if (gusty) {
    sentences.push(`Rachas de hasta ${Math.max(...hours.slice(0, 24).map((hour) => hour.gust ?? 0))}\u00a0km/h.`);
  }
  return sentences;
}

/**
 * Total precipitation (mm) in the next hours.
 *
 * @param {Hour[]} hours
 */
export function totalPrecipitation(hours) {
  return hours.reduce(
    (total, hour) =>
      total + (hour.rain === "Ip" ? 0 : amount(hour.rain)) + (hour.snow === "Ip" ? 0 : amount(hour.snow)),
    0,
  );
}
