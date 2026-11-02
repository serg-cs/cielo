/**
 * Detail sheets: one day, and the warnings of a place.
 */

import {
  WARNING_LEVELS,
  WIND_LEVEL_LIMITS,
  fireDanger,
  sky,
  uvCategory,
  warningPhenomenon,
  windDirection,
  windLevel,
  windStrength,
} from "./codes.js";
import { TEMPERATURE_STOPS, scale, sunBar, temperatureRange, tile, warningOnDate, warningTiming } from "./cards.js";
import { chart, chartRange } from "./chart.js";
import { h, icon, infoButton, windArrow } from "./dom.js";
import {
  amount,
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
} from "./format.js";
import { dayParts, daySummary, hoursOfDay } from "./forecast.js";

/** @typedef {import("./forecast.js").Forecast} Forecast */
/** @typedef {import("./forecast.js").Day} Day */
/** @typedef {import("./warnings.js").Warning} Warning */

/**
 * @param {string} title
 * @param {unknown} content
 * @param {() => void} onClose
 */
function sheet(title, content, onClose) {
  return [
    h(
      "header.sheet-header",
      null,
      h("span.sheet-handle", { "aria-hidden": "true" }),
      h("h2.sheet-title", { id: "sheet-title" }, title),
      h("button.icon-button", { type: "button", "aria-label": "Cerrar", onclick: onClose }, icon("x")),
    ),
    h("div.sheet-body", null, content),
  ];
}

/**
 * Details of one day.
 *
 * @param {Forecast} forecast
 * @param {string} date
 * @param {number} now
 * @param {() => void} onClose
 * @param {Warning[]} warnings Warnings of the place; those touching this day are shown.
 * @param {string} warningsHref Link to the place's warnings.
 */
export function daySheet(forecast, date, now, onClose, warnings, warningsHref) {
  const timeZone = forecast.timeZone;
  const today = localDate(now, timeZone);
  const day = forecast.days.find((candidate) => candidate.date === date);
  const title = `${dayName(date, today)}, ${dayMonth(date)}`;
  if (!day) {
    return sheet(title, h("p", null, "No hay previsión para este día."), onClose);
  }
  const summary = daySummary(day, {
    today,
    hour: date === today ? localHour(now, timeZone) : -1,
  });
  const condition = sky(summary.sky);

  // Warnings for this day, each opening the warnings sheet.
  const todays = warnings.filter((warning) => warningOnDate(warning, date, timeZone));
  const dayWarnings = todays.length
    ? h(
        "ul.day-warnings",
        { role: "list" },
        todays.map((warning) => {
          const phenomenon = warningPhenomenon(warning);
          return h(
            "li",
            null,
            h(
              "a.warning-banner",
              {
                href: warningsHref,
                class: `warning-banner level-${warning.level}`,
              },
              h("span.warning-banner-icon", null, icon(phenomenon.icon)),
              h(
                "span.warning-banner-text",
                null,
                h(
                  "strong",
                  null,
                  `Aviso ${WARNING_LEVELS[warning.level].label.toLowerCase()}: ${phenomenon.name.toLowerCase()}`,
                ),
                h("span", null, warningTiming(warning, timeZone, now)),
              ),
              icon("chevron-right", "icon icon-small"),
            ),
          );
        }),
      )
    : null;

  const parts = dayParts(day);
  const partsSection = parts.length
    ? h(
        "section.sheet-section",
        null,
        h("h3", null, "Resumen del día"),
        h(
          "ul.parts",
          { role: "list", style: { "--parts": parts.length } },
          parts.map(({ period, label, values }) => {
            const name = h(
              "span.part-name",
              null,
              label,
              h("small", null, `${period.slice(0, 2)}–${period.slice(3)}\u00a0h`),
            );
            // A part of today that is over: said plainly instead of empty values.
            if (date === today && Number(period.slice(3)) <= localHour(now, timeZone)) {
              return h(
                "li.part.past",
                null,
                name,
                h("span.part-past-icon", { "aria-hidden": "true" }, icon("clock", "icon")),
                h("span.part-past", null, "Ya ha pasado"),
              );
            }
            const partCondition = sky(values.sky);
            const direction = windDirection(values.wdir);
            const temperature = day.temps?.[String(Number(period.slice(3)) || 24)];
            return h(
              "li.part",
              null,
              name,
              h(
                "span.part-icon",
                { title: partCondition.label },
                values.sky ? icon(partCondition.icon, "icon icon-large") : "—",
              ),
              h("span.visually-hidden", null, values.sky ? partCondition.label : "Sin datos de cielo"),
              temperature !== undefined
                ? h(
                    "span.part-temp",
                    {
                      title: `Temperatura a las ${Number(period.slice(3)) || 24}\u00a0h`,
                    },
                    degrees(temperature),
                  )
                : null,
              h("span.part-pop", null, icon("umbrella", "icon icon-tiny"), values.pop != null ? `${values.pop}%` : "—"),
              values.wind != null
                ? h(
                    "span.part-wind",
                    { class: `part-wind wind-level-${windLevel(values.wind) ?? 0}` },
                    direction.degrees === null || values.wind < 6
                      ? icon("wind", "icon icon-tiny")
                      : windArrow(direction.degrees, "icon icon-tiny"),
                    values.wind < 6 ? "Calma" : `${values.wind}`,
                    values.wind < 6 ? null : h("small", null, " km/h"),
                  )
                : null,
              values.gust ? h("span.part-gust", null, `Rachas ${values.gust}\u00a0km/h`) : null,
            );
          }),
        ),
      )
    : null;

  const hours = hoursOfDay(forecast, date).filter((hour) => hour.temp !== null);
  const hoursSection =
    hours.length >= 3
      ? h(
          "section.sheet-section",
          null,
          h("h3", null, "Por horas"),
          h("p.sheet-hint", null, icon("pointer", "icon icon-small"), "Toca un gráfico para ver cada hora"),
          dayCharts(
            hours,
            timeZone,
            date === today ? localHour(now, timeZone) + Number(clockTime(now, timeZone).slice(3, 5)) / 60 : null,
          ),
        )
      : null;

  const details = dayTiles(forecast, day, summary, hours, date === today ? now : null);
  return sheet(
    title,
    [
      h(
        "div.sheet-summary",
        null,
        h("span.sheet-summary-icon", null, icon(condition.icon, "icon icon-hero")),
        h(
          "div",
          null,
          h("p.sheet-condition", null, summary.sky ? condition.label : "Sin datos de cielo"),
          h(
            "p.sheet-range",
            null,
            h("span", null, h("span.range-label", null, "Mín "), degrees(day.tmin)),
            h("span", null, h("span.range-label", null, "Máx "), degrees(day.tmax)),
          ),
          summary.pop !== null
            ? h(
                "p.sheet-pop",
                { title: "Probabilidad de precipitación" },
                icon("umbrella", "icon icon-small"),
                h("span.visually-hidden", null, "Probabilidad de precipitación "),
                `${summary.pop}%`,
              )
            : null,
        ),
      ),
      dayWarnings,
      partsSection,
      hoursSection,
      details.length
        ? h("section.sheet-section", null, h("h3", null, "Detalles"), h("ul.tiles", { role: "list" }, details))
        : null,
    ],
    onClose,
  );
}

/**
 * Detail tiles of one day, like the "Hoy en detalle" card.
 *
 * @param {Forecast} forecast
 * @param {Day} day
 * @param {ReturnType<typeof daySummary>} summary
 * @param {import("./forecast.js").Hour[]} hours Hours of the day.
 * @param {number | null} now Now, when the day is today.
 */
function dayTiles(forecast, day, summary, hours, now) {
  const tiles = [];

  // Strongest wind of the day, from the hours or else the day's periods.
  const windiest = hours.reduce(
    (best, hour) => (hour.wind !== null && (best === null || hour.wind > (best.wind ?? 0)) ? hour : best),
    /** @type {import("./forecast.js").Hour | null} */ (null),
  );
  const wind = windiest?.wind ?? summary.wind;
  if (wind !== null && wind !== undefined) {
    const direction = windDirection(windiest?.wdir ?? summary.wdir);
    const gusts = hours.map((hour) => hour.gust).filter((gust) => typeof gust === "number");
    const gust = gusts.length ? Math.max(.../** @type {number[]} */ (gusts)) : summary.gust;
    tiles.push(
      tile({
        iconName: "wind",
        guide: "viento",
        title: "Viento máximo",
        value: [h("span.value-number", null, wind), h("span.value-unit", null, " km/h")],
        visual: h(
          "span.compass",
          { class: `compass wind-level-${windLevel(wind) ?? 0}`, "aria-hidden": "true" },
          direction.degrees === null || wind < 6
            ? h("span.compass-calm", null, wind < 6 ? "Calma" : direction.short || "")
            : windArrow(direction.degrees, "icon"),
        ),
        detail: [
          wind < 6
            ? "En calma todo el día"
            : `${windStrength(wind)}${direction.degrees !== null ? ` del ${direction.label}` : ""}`,
          gust ? `. Rachas de hasta ${gust}\u00a0km/h` : "",
        ].join(""),
        className: "tile-wind",
      }),
    );
  }

  const total = hours.reduce((sum, hour) => sum + amount(hour.rain) + amount(hour.snow), 0);
  const storm = Math.max(0, ...hours.map((hour) => hour.storm ?? 0));
  const snow = Math.max(0, ...hours.map((hour) => hour.snowProb ?? 0));
  if (summary.pop !== null) {
    const extras = [
      total >= 0.1 ? millimetres(Math.round(total * 10) / 10) : null,
      storm ? `tormenta ${storm}%` : null,
      snow ? `nieve ${snow}%` : null,
    ].filter(Boolean);
    tiles.push(
      tile({
        iconName: "umbrella",
        guide: "precipitacion",
        title: "Lluvia",
        value: [h("span.value-number", null, `${summary.pop}`), h("span.value-unit", null, "%")],
        detail: [
          now === null ? "Probabilidad en el día" : "Probabilidad en el resto del día",
          extras.length ? [h("br"), capitalize(extras.join(" · "))] : null,
        ],
      }),
    );
  }

  if (day.feels && day.feels[0] !== null && day.feels[1] !== null) {
    tiles.push(
      tile({
        iconName: "thermometer-sun",
        guide: "temperatura",
        title: "Sensación",
        value: temperatureRange(day.feels[0], day.feels[1], "value-number"),
      }),
    );
  }

  if (day.rh && day.rh[0] !== null && day.rh[1] !== null) {
    tiles.push(
      tile({
        iconName: "droplet",
        guide: "humedad",
        title: "Humedad",
        value: [h("span.value-number", null, `${day.rh[0]}–${day.rh[1]}`), h("span.value-unit", null, "%")],
        visual: h(
          "span.meter.meter-range",
          { "aria-hidden": "true" },
          h("span", {
            style: { "--start": `${day.rh[0]}%`, "--end": `${day.rh[1]}%` },
          }),
        ),
      }),
    );
  }

  const uv = uvCategory(day.uv);
  if (uv) {
    tiles.push(
      tile({
        iconName: "sun-dim",
        guide: "uv",
        title: "UV máximo",
        value: [h("span.value-number", null, day.uv), h("span.value-unit", null, ` ${uv.label.toLowerCase()}`)],
        visual: scale("uv", 5, uv.level),
        detail: uv.advice,
      }),
    );
  }

  if (day.sun?.[0] && day.sun?.[1]) {
    const [sunrise, sunset] = day.sun;
    tiles.push(
      tile({
        iconName: "sunrise",
        guide: "sol",
        title: "Sol",
        value: sunBar(sunrise, sunset, null),
        detail: `${duration(sunrise, sunset)} de luz`,
      }),
    );
  }

  const fire = fireDanger(day.fire);
  if (fire) {
    tiles.push(
      tile({
        iconName: "flame",
        guide: "incendios",
        title: "Incendios",
        value: h("span.value-text", null, fire.label),
        visual: scale("fire", 6, fire.level),
      }),
    );
  }

  if (summary.snowl !== null) {
    const altitude = forecast.meta.alt;
    tiles.push(
      tile({
        iconName: "mountain-snow",
        guide: "nieve",
        title: "Cota de nieve",
        value: [h("span.value-number", null, number(summary.snowl)), h("span.value-unit", null, " m")],
        detail: typeof altitude === "number" ? `Este municipio está a ${number(altitude)}\u00a0m` : null,
      }),
    );
  }
  return tiles;
}

/**
 * One warning: a header with the phenomenon, area and level, then aligned
 * rows for when, the threshold, the probability and what the level means.
 * AEMET's generic advice for the level is left out; it is the same on every
 * warning.
 *
 * @param {Warning} warning
 * @param {string} timeZone
 * @param {number} now
 */
function warningCard(warning, timeZone, now) {
  const level = WARNING_LEVELS[warning.level];
  const phenomenon = warningPhenomenon(warning);
  /** @type {Array<[string, string | undefined]>} */
  const rows = [
    ["Cuándo", warningTiming(warning, timeZone, now)],
    // The threshold is left out when it only repeats the phenomenon ("Tormentas").
    [
      "Umbral",
      warning.param && warning.param.toLowerCase() !== phenomenon.name.toLowerCase() ? warning.param : undefined,
    ],
    ["Probabilidad", warning.prob?.replaceAll("-", "–")],
    ["Riesgo", level.meaning],
  ];
  const description = warning.desc && warning.desc.replace(/\.$/, "") !== warning.param ? warning.desc : null;
  return h(
    "article.warning",
    { class: `warning level-${warning.level}` },
    h(
      "header.warning-head",
      null,
      h("span.warning-icon", null, icon(phenomenon.icon)),
      h("div.warning-heading", null, h("h3", null, phenomenon.name), warning.area ? h("p", null, warning.area) : null),
      h("span.level-chip", null, level.label),
    ),
    h(
      "dl.warning-rows",
      null,
      rows
        .filter(([, value]) => value)
        .map(([label, value]) => h("div.warning-row", null, h("dt", null, label), h("dd", null, value))),
    ),
    description ? h("p.warning-description", null, description) : null,
  );
}

/**
 * All warnings of a place.
 *
 * @param {Warning[]} warnings
 * @param {string} timeZone
 * @param {number} now
 * @param {() => void} onClose
 */
export function warningsSheet(warnings, timeZone, now, onClose) {
  if (warnings.length === 0) {
    return sheet("Avisos", h("p", null, "No hay avisos meteorológicos en vigor para este lugar."), onClose);
  }
  return sheet(
    warnings.length === 1 ? "Aviso meteorológico" : `${warnings.length} avisos meteorológicos`,
    [
      h(
        "ul.warning-list",
        { role: "list" },
        warnings.map((warning) => h("li", null, warningCard(warning, timeZone, now))),
      ),
      h(
        "p.sheet-note.with-info",
        null,
        "Avisos del plan Meteoalerta de AEMET para la zona de este lugar.",
        infoButton("avisos", "niveles de aviso"),
      ),
    ],
    onClose,
  );
}

/** Wind colours by speed: the severity scale, in hard steps. @type {Array<[number, string]>} */
const WIND_COLORS = [0, ...WIND_LEVEL_LIMITS].map((speed, level) => [speed, `var(--severity-${level})`]);

/**
 * Charts for the hours of one day: temperature, rain, wind and humidity.
 * Today's charts mark the current hour.
 *
 * @param {import("./forecast.js").Hour[]} hours
 * @param {string} timeZone
 * @param {number | null} now Current hour (0-24, fractional), on today's charts.
 */
function dayCharts(hours, timeZone, now) {
  const at = hours.map((hour) => localHour(hour.time, timeZone));
  const time = (/** @type {number} */ index) => `${at[index]}:00`;
  const present = (/** @type {Array<number | null>} */ values) =>
    /** @type {number[]} */ (values.filter((value) => value !== null));
  const temps = hours.map((hour) => hour.temp);
  const feels = hours.map((hour) => hour.feels);
  const rain = hours.map((hour) => amount(hour.rain) + amount(hour.snow));
  const pops = hours.map((hour) => hour.pop);
  const wind = hours.map((hour) => hour.wind);
  const gusts = hours.map((hour) => hour.gust);
  const humidity = hours.map((hour) => hour.rh);
  const total = rain.reduce((sum, value) => sum + value, 0);
  const snow = hours.some((hour) => amount(hour.snow) > 0);
  const showFeels = hours.some(
    (hour) => hour.feels !== null && hour.temp !== null && Math.abs(hour.feels - hour.temp) >= 2,
  );
  const rounded = (/** @type {number} */ value) => millimetres(value < 0.1 ? "Ip" : Math.round(value * 10) / 10);
  const rainClass = snow ? "snow" : "rain";

  const shared = { hours: at, now };

  const charts = [
    chart({
      ...shared,
      title: "Temperatura",
      iconName: "thermometer",
      summary: `mínima ${degrees(Math.min(...present(temps)))}, máxima ${degrees(Math.max(...present(temps)))}`,
      series: [
        ...(showFeels
          ? [
              {
                values: feels,
                kind: /** @type {const} */ ("line"),
                className: "feels",
              },
            ]
          : []),
        {
          values: temps,
          kind: /** @type {const} */ ("line"),
          className: "temperature",
          colors: TEMPERATURE_STOPS,
          area: true,
        },
      ],
      range: chartRange(showFeels ? [...temps, ...feels] : temps, {
        minimumSpan: 6,
        step: 2,
      }),
      describe: (index) => ({
        time: time(index),
        readings: [
          { value: degrees(temps[index]) },
          ...(showFeels && feels[index] !== null
            ? [
                {
                  value: degrees(feels[index]),
                  label: "sensación",
                  className: "feels",
                },
              ]
            : []),
        ],
      }),
      label: (value) => degrees(Math.round(value)),
      key: showFeels
        ? [
            ["temperature", "Temperatura"],
            ["feels", "Sensación"],
          ]
        : [],
    }),
  ];
  // Rain is always charted, so every day reads the same; a dry day shows
  // flat at zero.
  charts.push(
    chart({
      ...shared,
      title: "Lluvia",
      iconName: "umbrella",
      summary:
        total > 0
          ? `${rounded(total)} en el día`
          : `probabilidad de hasta ${Math.max(0, ...present(pops))}%`,
      series: [
        {
          values: pops,
          kind: "line",
          className: "probability",
          range: [0, 100],
          area: true,
        },
        { values: rain, kind: "bars", className: rainClass },
      ],
      range: chartRange(rain, { floor: 0, minimumSpan: 2, step: 2 }),
      describe: (index) => ({
        time: time(index),
        readings: [
          {
            value: rain[index] > 0 ? rounded(rain[index]) : "0\u00a0mm",
            // "Unas gotas" needs no "lluvia" after it.
            label: rain[index] > 0 && rain[index] < 0.1 ? undefined : snow ? "nieve" : "lluvia",
            className: rainClass,
          },
          ...(pops[index] !== null
            ? [
                {
                  value: `${pops[index]}%`,
                  label: "prob.",
                  className: "probability",
                },
              ]
            : []),
        ],
      }),
      label: (value) => number(Math.round(value * 10) / 10, value % 1 ? 1 : 0),
      unit: "mm",
      key: [
        [rainClass, snow ? "Nieve (l/m²)" : "Lluvia (l/m²)"],
        ["probability", "Probabilidad (0–100 %)"],
      ],
    }),
  );
  charts.push(
    chart({
      ...shared,
      title: "Viento",
      iconName: "wind",
      summary: `hasta ${Math.max(...present(wind), 0)} km/h${present(gusts).length ? `, rachas de ${Math.max(...present(gusts))} km/h` : ""}`,
      series: [
        { values: gusts, kind: "line", className: "gust", colors: WIND_COLORS, hard: true },
        { values: wind, kind: "line", className: "wind", colors: WIND_COLORS, hard: true },
      ],
      range: chartRange([...wind, ...gusts], {
        floor: 0,
        minimumSpan: 20,
        step: 10,
      }),
      // A direction every three hours, in the colour of its strength.
      arrows: hours
        .map((hour, index) => ({ hour, index }))
        .filter(({ index }) => at[index] % 3 === 1)
        .map(({ hour, index }) => ({
          index,
          degrees: (hour.wind ?? 0) < 6 ? null : windDirection(hour.wdir).degrees,
          level: windLevel(hour.wind),
        })),
      describe: (index) => {
        const direction = windDirection(hours[index].wdir);
        const calm = (wind[index] ?? 0) < 6;
        return {
          time: time(index),
          readings: [
            calm
              ? { value: "Calma", className: "wind" }
              : {
                  value: `${wind[index] ?? "—"}\u00a0km/h`,
                  className: "wind",
                  arrow: direction.degrees,
                },
            ...(gusts[index]
              ? [
                  {
                    value: `${gusts[index]}`,
                    label: "rachas",
                    className: "gust",
                  },
                ]
              : []),
          ],
        };
      },
      label: (value) => String(value),
      unit: "km/h",
      key: present(gusts).length
        ? [
            ["wind", "Viento medio"],
            ["gust", "Rachas"],
          ]
        : [],
    }),
  );
  if (present(humidity).length) {
    charts.push(
      chart({
        ...shared,
        title: "Humedad",
        iconName: "droplet",
        summary: `entre ${Math.min(...present(humidity))}% y ${Math.max(...present(humidity))}%`,
        series: [{ values: humidity, kind: "line", className: "humidity", area: true }],
        range: [0, 100],
        describe: (index) => ({
          time: time(index),
          readings: [{ value: `${humidity[index] ?? "—"}%` }],
        }),
        label: (value) => String(value),
        unit: "%",
      }),
    );
  }
  return h("div.charts", null, charts);
}
