/**
 * Cards of the municipality forecast screen, except the hourly chart.
 */

import {
  WARNING_LEVELS,
  beachSky,
  beachWaves,
  fireDanger,
  sky,
  uvCategory,
  warningPhenomenon,
  windDirection,
  windLevel,
  windStrength,
} from "./codes.js";
import { h, icon, infoButton, windArrow } from "./dom.js";
import {
  addDays,
  atClock,
  capitalize,
  clockTime,
  dayMonth,
  dayName,
  degrees,
  duration,
  localDate,
  localHour,
  millimetres,
  number,
  weekday,
} from "./format.js";
import { daySummary, totalPrecipitation, upcomingHours, upcomingRisks } from "./forecast.js";

/** @typedef {import("./forecast.js").Forecast} Forecast */
/** @typedef {import("./forecast.js").Hour} Hour */
/** @typedef {import("./forecast.js").Day} Day */
/** @typedef {import("./warnings.js").Warning} Warning */

/**
 * A card with a titled header.
 *
 * @param {string} id
 * @param {string} iconName
 * @param {string} title
 * @param {unknown[]} content
 * @param {string} [className]
 * @param {string} [guide] Guide section the title's ⓘ opens.
 */
export function card(id, iconName, title, content, className = "", guide) {
  return h(
    "section.card",
    { class: `card ${className}`.trim(), "aria-labelledby": id },
    h(
      "div.card-header",
      null,
      h(
        "h2.card-title",
        { id },
        icon(iconName, "icon icon-small"),
        title,
        guide ? infoButton(guide, title.toLowerCase()) : null,
      ),
    ),
    content,
  );
}

/**
 * `hoy`, `mañana` or `el lunes`.
 *
 * @param {string} date
 * @param {string} today
 */
function dayPhrase(date, today) {
  if (date === today) return "hoy";
  if (date === addDays(today, 1)) return "mañana";
  return `el ${weekday(date)}`;
}

/**
 * When a warning applies, in words: `En vigor hasta hoy a las 23:59`,
 * `Mañana de 12:00 a 23:59`, `Desde hoy a las 18:00 hasta mañana a las 06:00`.
 *
 * @param {Warning} warning
 * @param {string} timeZone
 * @param {number} now
 */
export function warningTiming(warning, timeZone, now) {
  const today = localDate(now, timeZone);
  const onset = warning.onset ? Date.parse(warning.onset) : Number.NaN;
  const expires = warning.expires ? Date.parse(warning.expires) : Number.NaN;
  const at = (/** @type {number} */ time) =>
    `${dayPhrase(localDate(time, timeZone), today)} ${atClock(time, timeZone)}`;
  const started = !Number.isFinite(onset) || onset <= now;
  if (started) return Number.isFinite(expires) ? `En vigor hasta ${at(expires)}` : "En vigor";
  if (!Number.isFinite(expires)) return `Desde ${at(onset)}`;
  const onsetDate = localDate(onset, timeZone);
  if (onsetDate === localDate(expires, timeZone)) {
    const clock = (/** @type {number} */ time) => `${localHour(time, timeZone)}:${clockTime(time, timeZone).slice(3)}`;
    return capitalize(`${dayPhrase(onsetDate, today)} de ${clock(onset)} a ${clock(expires)}`);
  }
  return `Desde ${at(onset)} hasta ${at(expires)}`;
}

/**
 * Banner with the most severe warning; it opens the warnings sheet.
 *
 * @param {Warning[]} warnings
 * @param {string} timeZone
 * @param {number} now
 * @param {string} href
 */
export function warningsBanner(warnings, timeZone, now, href) {
  if (warnings.length === 0) return null;
  const top = warnings[0];
  const level = WARNING_LEVELS[top.level];
  const phenomenon = warningPhenomenon(top);
  const others = warnings.length - 1;
  return h(
    "a.warning-banner",
    { href, class: `warning-banner level-${top.level}` },
    h("span.warning-banner-icon", null, icon(phenomenon.icon)),
    h(
      "span.warning-banner-text",
      null,
      h("strong", null, `Aviso ${level.label.toLowerCase()}: ${phenomenon.name.toLowerCase()}`),
      h(
        "span",
        null,
        warningTiming(top, timeZone, now),
        others > 0 ? ` · ${others === 1 ? "1 aviso más" : `${others} avisos más`}` : "",
      ),
    ),
    icon("chevron-right", "icon icon-small"),
  );
}

/**
 * Current conditions: the forecast for this hour, today's range and a
 * short description of the coming hours.
 *
 * @param {object} options
 * @param {Hour | null} options.hour
 * @param {Day | null} options.today
 * @param {string[]} options.summary Sentences about the coming hours.
 * @param {string | null} options.status Message when there is no data.
 */
export function nowCard({ hour, today, summary, status }) {
  if (!hour) {
    return h(
      "section.card.now.now-empty",
      { "aria-live": "polite" },
      h("p", null, status ?? "Previsión no disponible"),
    );
  }
  const condition = sky(hour.sky);
  const range = [
    today?.tmin != null ? h("span", null, h("span.range-label", null, "Mín "), degrees(today.tmin)) : null,
    today?.tmax != null ? h("span", null, h("span.range-label", null, "Máx "), degrees(today.tmax)) : null,
  ].filter(Boolean);
  return h(
    "section.card.now",
    { "aria-label": "Ahora" },
    h(
      "div.now-main",
      null,
      h(
        "div.now-reading",
        null,
        h("p.now-temperature", null, degrees(hour.temp)),
        h("p.now-condition", null, condition.label),
        range.length ? h("p.now-range", null, range) : null,
      ),
      h("span.now-icon", null, icon(condition.icon, "icon icon-hero")),
    ),
    summary.length ? h("p.now-summary", null, summary.join(" ")) : null,
  );
}

/** Forecasts older than this are flagged, since AEMET updates them several times a day. */
const STALE_AFTER = 6 * 3600 * 1000;

/**
 * Note when AEMET issued the forecast more than 6 hours ago, or `null`.
 *
 * @param {number} issued When AEMET issued the forecast (ms), or 0 when unknown.
 * @param {number} now
 */
export function staleNote(issued, now) {
  if (!issued || now - issued < STALE_AFTER) return null;
  return "Previsión elaborada hace más de 6\u00a0horas";
}

/**
 * Next days with min-max range bars. Each row opens the day sheet.
 *
 * @param {Day[]} days
 * @param {{today: string, hour: number, timeZone: string, warnings: Warning[], hrefFor: (date: string) => string}} context
 */
export function dailyCard(days, { today, hour, timeZone, warnings, hrefFor }) {
  if (days.length === 0) return null;
  const minimums = /** @type {number[]} */ (days.map((day) => day.tmin).filter((value) => value !== null));
  const maximums = /** @type {number[]} */ (days.map((day) => day.tmax).filter((value) => value !== null));
  const low = Math.min(...minimums, ...maximums);
  const high = Math.max(...maximums, ...minimums);
  const span = Math.max(high - low, 1);
  const gradient = temperatureGradient(low, high);

  const rows = days.map((day) => {
    const summary = daySummary(day, { today, hour });
    const condition = sky(summary.sky);
    let start = day.tmin === null ? 0 : ((day.tmin - low) / span) * 100;
    let end = day.tmax === null ? 100 : ((day.tmax - low) / span) * 100;
    if (end - start < 6) {
      // Keep very small ranges visible.
      const middle = (start + end) / 2;
      start = Math.max(0, Math.min(94, middle - 3));
      end = start + 6;
    }
    const dayWarnings = warnings.filter((warning) => warningOnDate(warning, day.date, timeZone));
    const warning = dayWarnings[0] ?? null;
    const spoken = [
      dayName(day.date, today),
      dayMonth(day.date),
      summary.sky ? condition.label : null,
      summary.pop ? `probabilidad de precipitación ${summary.pop}%` : null,
      day.tmin !== null ? `mínima ${day.tmin} grados` : null,
      day.tmax !== null ? `máxima ${day.tmax} grados` : null,
      warning ? `aviso ${WARNING_LEVELS[warning.level].label.toLowerCase()}` : null,
    ]
      .filter(Boolean)
      .join(", ");
    return h(
      "li",
      null,
      h(
        "a.day-row",
        { href: hrefFor(day.date), "aria-label": spoken },
        h(
          "span.day-name",
          { "aria-hidden": "true" },
          h("strong", null, dayName(day.date, today, "short")),
          h(
            "small",
            null,
            dayMonth(day.date, "short"),
            // A warning is marked by the date, away from the weather icon.
            warning
              ? h(
                  "span.day-warning",
                  { class: `day-warning level-${warning.level}` },
                  icon("triangle-alert", "icon icon-tiny"),
                )
              : null,
          ),
        ),
        // The chance of rain sits under the sky, as in most weather apps.
        h(
          "span.day-sky",
          { "aria-hidden": "true" },
          icon(condition.icon, "icon icon-condition"),
          summary.pop ? h("span.day-pop", null, `${summary.pop}%`) : null,
        ),
        h(
          "span.day-range",
          { "aria-hidden": "true" },
          h("span.day-min", null, degrees(day.tmin)),
          h(
            "span.range-track",
            null,
            h("span.range-fill", {
              style: {
                "--start": `${start}%`,
                "--end": `${end}%`,
                "--gradient": gradient,
              },
            }),
          ),
          h("span.day-max", null, degrees(day.tmax)),
        ),
        icon("chevron-right", "icon icon-small day-chevron"),
      ),
    );
  });

  return h(
    "section.card.daily",
    { "aria-labelledby": "daily-title" },
    h(
      "div.card-header",
      null,
      h(
        "h2.card-title",
        { id: "daily-title" },
        icon("calendar-days", "icon icon-small"),
        `Próximos ${days.length} días`,
      ),
    ),
    h("ul.day-list", { role: "list" }, rows),
  );
}

/**
 * Whether a warning covers part of a local date.
 *
 * @param {Warning} warning
 * @param {string} date
 * @param {string} timeZone
 */
export function warningOnDate(warning, date, timeZone) {
  const onset = warning.onset ? localDate(Date.parse(warning.onset), timeZone) : "0000-00-00";
  const expires = warning.expires ? localDate(Date.parse(warning.expires) - 1, timeZone) : "9999-99-99";
  return onset <= date && date <= expires;
}

/**
 * A segmented scale, such as UV or fire danger, with the current level marked.
 *
 * @param {string} kind `uv` or `fire`.
 * @param {number} levels
 * @param {number} level
 */
export function scale(kind, levels, level) {
  return h(
    "span.scale",
    { class: `scale scale-${kind}`, "aria-hidden": "true" },
    Array.from({ length: levels }, (_, index) =>
      h("span", {
        class: `level-${index + 1}${index + 1 === level ? " current" : ""}`,
      }),
    ),
  );
}

/**
 * Sunrise and sunset under their icons, with a daylight bar between them
 * filled up to now when `progress` (0-100) is given.
 *
 * @param {string} sunrise `HH:MM`
 * @param {string} sunset `HH:MM`
 * @param {number | null} progress
 */
export function sunBar(sunrise, sunset, progress) {
  return h(
    "span.sun-bar",
    null,
    h("span.sun-end", null, icon("sunrise", "icon"), h("span.visually-hidden", null, "Amanecer "), sunrise),
    h("span.sun-end.end", null, icon("sunset", "icon"), h("span.visually-hidden", null, "Atardecer "), sunset),
    progress === null
      ? null
      : h("span.meter.daylight", { "aria-hidden": "true" }, h("span", { style: { "--value": `${progress}%` } })),
  );
}

/**
 * Colour stops of the temperature scale, in degrees, shared by every range
 * bar so a colour always means the same temperature.
 *
 * @type {Array<[number, string]>}
 */
export const TEMPERATURE_STOPS = [
  [-10, "var(--mono-purple)"],
  [0, "var(--mono-cyan)"],
  [10, "var(--mono-green)"],
  [20, "var(--mono-yellow)"],
  [30, "var(--mono-orange)"],
  [40, "var(--mono-red)"],
];

/**
 * Gradient for a bar that spans `low`..`high` degrees.
 *
 * @param {number} low
 * @param {number} high
 */
export function temperatureGradient(low, high) {
  const span = Math.max(high - low, 1);
  const stops = TEMPERATURE_STOPS.map(([degrees, color]) => `${color} ${(((degrees - low) / span) * 100).toFixed(1)}%`);
  return `linear-gradient(90deg, ${stops.join(", ")})`;
}

/**
 * Lowest and highest temperature with a bar in the temperature colours
 * between them.
 *
 * @param {number} low
 * @param {number} high
 * @param {string} [className] Class of the two values, such as `value-number`.
 */
export function temperatureRange(low, high, className = "") {
  const value = (/** @type {string} */ label, /** @type {number} */ degreesValue) =>
    h("span", { class: className || null }, h("span.visually-hidden", null, `${label} `), degrees(degreesValue));
  return h(
    "span.temp-range",
    null,
    value("mínima", low),
    h("span.temp-range-bar", {
      style: { background: temperatureGradient(low, Math.max(high, low + 1)) },
      "aria-hidden": "true",
    }),
    value("máxima", high),
  );
}

/**
 * One detail tile.
 *
 * @param {object} options
 * @param {string} options.iconName
 * @param {string} options.title
 * @param {unknown} options.value
 * @param {unknown} [options.visual]
 * @param {unknown} [options.detail]
 * @param {string} [options.className]
 * @param {string} [options.guide] Guide section the title's ⓘ opens.
 */
export function tile({ iconName, title, value, visual, detail, className, guide }) {
  return h(
    "li.tile",
    { class: ["tile", className].filter(Boolean).join(" ") },
    h(
      "h3.tile-title",
      null,
      icon(iconName, "icon icon-small"),
      title,
      guide ? infoButton(guide, title.toLowerCase()) : null,
    ),
    h("p.tile-value", null, value),
    visual ?? null,
    detail ? h("p.tile-detail", null, detail) : null,
  );
}

/**
 * Detail tiles for today, each with a short explanation.
 *
 * @param {Forecast} forecast
 * @param {Hour | null} hour
 * @param {Day | null} today
 * @param {number} now
 */
export function detailsCard(forecast, hour, today, now) {
  const tiles = [];
  const timeZone = forecast.timeZone;
  const todayDate = localDate(now, timeZone);
  const summary = today ? daySummary(today, { today: todayDate, hour: localHour(now, timeZone) }) : null;

  if (hour && hour.wind !== null) {
    const direction = windDirection(hour.wdir);
    const gust = hour.gust ?? summary?.gust ?? null;
    tiles.push(
      tile({
        iconName: "wind",
        guide: "viento",
        title: "Viento",
        value: [h("span.value-number", null, hour.wind), h("span.value-unit", null, " km/h")],
        visual: h(
          "span.compass",
          { class: `compass wind-level-${windLevel(hour.wind) ?? 0}`, "aria-hidden": "true" },
          direction.degrees === null || hour.wind < 6
            ? h("span.compass-calm", null, hour.wind < 6 ? "Calma" : direction.short || "")
            : windArrow(direction.degrees, "icon"),
        ),
        detail: [
          hour.wind < 6
            ? "En calma"
            : `${windStrength(hour.wind)}${direction.degrees !== null ? ` del ${direction.label}` : direction.label === "variable" ? ", de dirección variable" : ""}`,
          gust ? `. Rachas de hasta ${gust}\u00a0km/h` : "",
        ].join(""),
        className: "tile-wind",
      }),
    );
  }

  const next24 = upcomingHours(forecast, now, 24);
  if (next24.length > 0) {
    const risks = upcomingRisks(forecast, now);
    const total = totalPrecipitation(next24);
    tiles.push(
      tile({
        iconName: "umbrella",
        guide: "precipitacion",
        title: "Lluvia",
        value: [h("span.value-number", null, `${risks.pop}`), h("span.value-unit", null, "%")],
        detail: (() => {
          const extras = [
            total >= 0.1 ? millimetres(Math.round(total * 10) / 10) : null,
            risks.storm ? `tormenta ${risks.storm}%` : null,
            risks.snow ? `nieve ${risks.snow}%` : null,
          ].filter(Boolean);
          return ["Probabilidad en 24\u00a0h", extras.length ? [h("br"), capitalize(extras.join(" · "))] : null];
        })(),
      }),
    );
  }

  if (hour && hour.rh !== null) {
    tiles.push(
      tile({
        iconName: "droplet",
        guide: "humedad",
        title: "Humedad",
        value: [h("span.value-number", null, hour.rh), h("span.value-unit", null, "%")],
        visual: h("span.meter", { "aria-hidden": "true" }, h("span", { style: { "--value": `${hour.rh}%` } })),
        detail: today?.rh?.[0] != null && today.rh[1] != null ? `Hoy entre ${today.rh[0]}% y ${today.rh[1]}%` : null,
      }),
    );
  }

  const uvDay = today?.uv != null ? today : forecast.days.find((day) => day.date >= todayDate && day.uv != null);
  const uv = uvCategory(uvDay?.uv);
  if (uvDay && uv) {
    tiles.push(
      tile({
        iconName: "sun-dim",
        guide: "uv",
        title: uvDay === today ? "UV máximo" : `UV ${dayName(uvDay.date, todayDate).toLowerCase()}`,
        value: [h("span.value-number", null, uvDay.uv), h("span.value-unit", null, ` ${uv.label.toLowerCase()}`)],
        visual: scale("uv", 5, uv.level),
        detail: uv.advice,
      }),
    );
  }

  if (today?.sun && today.sun[0] && today.sun[1]) {
    const [sunrise, sunset] = today.sun;
    const minutes = (/** @type {string} */ time) => Number(time.slice(0, 2)) * 60 + Number(time.slice(3, 5));
    const nowMinutes = localHour(now, timeZone) * 60 + Number(clockTime(now, timeZone).slice(3, 5));
    const progress = Math.min(
      100,
      Math.max(0, ((nowMinutes - minutes(sunrise)) / (minutes(sunset) - minutes(sunrise))) * 100),
    );
    tiles.push(
      tile({
        iconName: "sunrise",
        guide: "sol",
        title: "Sol",
        value: sunBar(sunrise, sunset, progress),
        detail: `${duration(sunrise, sunset)} de luz hoy`,
      }),
    );
  }

  const fire = fireDanger(today?.fire);
  if (fire) {
    tiles.push(
      tile({
        iconName: "flame",
        guide: "incendios",
        title: "Incendios",
        value: h("span.value-text", null, fire.label),
        visual: scale("fire", 6, fire.level),
        detail: "Riesgo de incendio forestal hoy",
      }),
    );
  }

  const snowLevels = forecast.days
    .filter((day) => day.date >= todayDate && day.date <= addDays(todayDate, 2))
    .flatMap((day) => Object.values(day.periods).map((period) => period.snowl))
    .filter((level) => typeof level === "number");
  if (snowLevels.length > 0) {
    const level = Math.min(.../** @type {number[]} */ (snowLevels));
    const altitude = /** @type {number | undefined} */ (forecast.meta.alt);
    tiles.push(
      tile({
        iconName: "mountain-snow",
        guide: "nieve",
        title: "Cota de nieve",
        value: [h("span.value-number", null, number(level)), h("span.value-unit", null, " m")],
        detail:
          typeof altitude === "number"
            ? `Mínima en 3 días. Este municipio está a ${number(altitude)}\u00a0m`
            : "Mínima en los próximos 3 días",
      }),
    );
  }

  if (tiles.length === 0) return null;
  return card("details-title", "gauge", "Hoy en detalle", [h("ul.tiles", { role: "list" }, tiles)], "details");
}

/**
 * Links to the beaches of a municipality, with today's water temperature.
 *
 * @param {Array<{id: string, name: string, days?: any[]}>} beaches
 * @param {string} today
 * @param {number} hour Local hour, to pick morning or afternoon.
 * @param {string} municipality
 */
export function beachLinksCard(beaches, today, hour, municipality) {
  if (beaches.length === 0) return null;
  return card(
    "beaches-title",
    "parasol",
    beaches.length === 1 ? `Playa de ${municipality}` : `Playas de ${municipality}`,
    [
      h(
        "ul.link-list",
        { role: "list" },
        beaches.map((beach) => {
          const day = (beach.days ?? []).find((/** @type {any} */ candidate) => candidate.date === today) ?? null;
          const half = hour < 14 ? 0 : 1;
          const condition = beachSky(day?.sky?.[half]);
          const waves = beachWaves(day?.waves?.[half]);
          return h(
            "li",
            null,
            h(
              "a.link-row",
              { href: `#/p/${beach.id}` },
              h(
                "span.link-text",
                null,
                h("span.link-name", null, beach.name),
                day
                  ? h(
                      "span.link-detail",
                      null,
                      [condition.label, waves ? `oleaje ${waves.label.toLowerCase()}` : null]
                        .filter(Boolean)
                        .join(" · "),
                    )
                  : h("span.link-detail", null, "Ver previsión de la playa"),
              ),
              h(
                "span.link-side",
                null,
                day ? icon(condition.icon, "icon icon-condition") : null,
                day?.water != null
                  ? h(
                      "span.water-chip",
                      { title: "Temperatura del agua" },
                      icon("waves", "icon icon-tiny"),
                      h("span.visually-hidden", null, "Agua "),
                      degrees(day.water),
                    )
                  : null,
                icon("chevron-right", "icon icon-small link-chevron"),
              ),
            ),
          );
        }),
      ),
    ],
    "beach-links",
  );
}

/**
 * Note shown when the device is offline and the data may be old.
 *
 * @param {boolean} offline
 */
export function offlineNote(offline) {
  return offline
    ? h(
        "p.offline-note",
        { role: "status" },
        icon("wifi-off", "icon icon-small"),
        "Sin conexión: se muestran los últimos datos guardados",
      )
    : null;
}
