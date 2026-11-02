/**
 * Next hours: one column per hour (time and sky) and a toggle that picks
 * what the columns show: temperature, rain or wind.
 */

import { sky, windDirection, windLevel } from "./codes.js";
import { smoothPath } from "./chart.js";
import { h, icon, svg, windArrow } from "./dom.js";
import { amount, dayName, degrees, hourLabel, localDate, millimetres, number, relativeHour } from "./format.js";

/** @typedef {import("./forecast.js").Hour} Hour */

const MODES = [
  {
    value: "temp",
    label: "Temperatura",
    short: "Temp.",
    spoken: "Temperatura",
    caption: null,
  },
  {
    value: "rain",
    label: "Lluvia",
    short: "Lluvia",
    spoken: "Lluvia",
    caption: "Probabilidad de lluvia en cada tramo de 6\u00a0horas y, debajo, litros por m² previstos en esa hora.",
  },
  {
    value: "wind",
    label: "Viento",
    short: "Viento",
    spoken: "Viento",
    caption: "Viento medio en km/h y, debajo, la racha máxima prevista.",
  },
];

/**
 * Relative position (0 lowest, 1 highest) of each value, for the small
 * rise and fall of temperatures across the strip.
 *
 * @param {Array<number | null>} values
 * @param {number} [minimumSpan]
 * @returns {Array<number | null>}
 */
export function relativeLevels(values, minimumSpan = 6) {
  const present = /** @type {number[]} */ (values.filter((value) => value !== null));
  if (present.length === 0) return values.map(() => null);
  let low = Math.min(...present);
  let high = Math.max(...present);
  if (high - low < minimumSpan) {
    const middle = (high + low) / 2;
    low = middle - minimumSpan / 2;
    high = middle + minimumSpan / 2;
  }
  return values.map((value) => (value === null ? null : (value - low) / (high - low)));
}

/**
 * The three readings of one hour; the card shows the one for its mode.
 *
 * @param {Hour} hour
 * @param {number | null} level Relative temperature (0-1).
 */
function readings(hour, level) {
  const pop = hour.pop;
  const total = amount(hour.rain) + amount(hour.snow);
  const direction = windDirection(hour.wdir);
  return [
    h(
      "span.reading.reading-temp",
      { style: level === null ? null : { "--level": level.toFixed(3) } },
      h("span.reading-value", null, degrees(hour.temp)),
    ),
    h(
      "span.reading.reading-rain",
      null,
      h(
        "span.rain-column",
        null,
        h("span.rain-fill", {
          class: amount(hour.snow) > 0 ? "rain-fill snow" : "rain-fill",
          style: { "--value": `${pop ?? 0}%` },
        }),
      ),
      h(
        "span.reading-value",
        { class: pop ? "reading-value rain-text" : "reading-value muted" },
        pop === null ? "—" : `${pop}%`,
      ),
      h("span.reading-extra", null, total > 0 ? (total < 0.1 ? "gotas" : number(total, 1)) : ""),
    ),
    h(
      "span.reading.reading-wind",
      { class: `reading reading-wind wind-level-${windLevel(hour.wind) ?? 0}` },
      direction.degrees === null || (hour.wind ?? 0) < 6
        ? h("span.wind-calm", null, hour.wind === null ? "" : hour.wind < 6 ? "Calma" : direction.short || "")
        : windArrow(direction.degrees, "icon"),
      h("span.reading-value", null, hour.wind === null ? "—" : hour.wind),
      h("span.reading-extra", null, hour.gust ? hour.gust : ""),
    ),
  ];
}

/**
 * Vertical position (0-100 % of the reading row) of the temperature dot
 * for a relative level. It mirrors the CSS that places the value above it.
 *
 * @param {number} level
 */
function dotPosition(level) {
  // In rem: offset (1 - level) × 2.25 + 0.25, value line 1.275, gap 0.25,
  // half the dot 0.25; the row is 4.5 tall.
  return (((1 - level) * 2.25 + 2.025) / 4.5) * 100;
}

/**
 * @param {object} options
 * @param {Hour[]} options.hours Upcoming hours, the first one being now.
 * @param {string} options.timeZone
 * @param {number} options.now
 * @param {string} options.mode
 * @param {(mode: string) => void} options.onModeChange
 * @param {number} [options.scrollLeft]
 */
export function hourlyCard({ hours, timeZone, now, mode, onModeChange, scrollLeft = 0 }) {
  let current = MODES.find((option) => option.value === mode) ?? MODES[0];
  const caption = h("p.hourly-caption", { hidden: !current.caption }, current.caption ?? "");
  /** @type {HTMLElement | null} */
  let card = null;
  /** @type {HTMLElement | null} */
  let scrollerElement = null;
  // Switching only changes which reading shows: hours, icons and position stay.
  const select = (/** @type {typeof MODES[number]} */ option) => {
    current = option;
    card?.classList.remove("mode-temp", "mode-rain", "mode-wind");
    card?.classList.add(`mode-${option.value}`);
    caption.hidden = !option.caption;
    caption.textContent = option.caption ?? "";
    scrollerElement?.setAttribute("aria-label", `${option.spoken} por horas, desplazable`);
    onModeChange(option.value);
  };
  const toggle = h(
    "div.segmented.segmented-fill",
    { role: "radiogroup", "aria-label": "Qué mostrar por horas" },
    MODES.map((option) =>
      h(
        "label.segment",
        null,
        h("input", {
          type: "radio",
          name: "hourly-mode",
          value: option.value,
          checked: option.value === current.value,
          "aria-label": option.spoken,
          onchange: () => select(option),
        }),
        h(
          "span",
          { "aria-hidden": "true" },
          h("span.segment-long", null, option.label),
          h("span.segment-short", null, option.short),
        ),
      ),
    ),
  );
  const header = h(
    "div.card-header",
    null,
    h("h2.card-title", { id: "hourly-title" }, icon("clock", "icon icon-small"), "Por horas"),
    toggle,
  );
  if (hours.length === 0) {
    return h(
      "section.card.hourly",
      { "aria-labelledby": "hourly-title" },
      header,
      h("p.card-empty", null, "No hay previsión por horas para este lugar."),
    );
  }

  const today = localDate(now, timeZone);
  const levels = relativeLevels(hours.map((hour) => hour.temp));
  const items = hours.map((hour, index) => {
    const date = localDate(hour.time, timeZone);
    const firstOfDay = index > 0 && date !== localDate(hours[index - 1].time, timeZone);
    const condition = sky(hour.sky);
    const direction = windDirection(hour.wdir);
    const rain = amount(hour.rain) + amount(hour.snow);
    const spoken = [
      index === 0 ? "Ahora" : relativeHour(hour.time, timeZone, today),
      hour.sky ? condition.label : null,
      hour.temp !== null ? `${hour.temp} grados` : null,
      hour.pop !== null ? `probabilidad de precipitación ${hour.pop}%` : null,
      rain > 0 ? millimetres(hour.rain === "Ip" && !amount(hour.snow) ? "Ip" : rain) : null,
      hour.wind !== null ? `viento ${direction.label} ${hour.wind} km/h` : null,
      hour.gust ? `rachas ${hour.gust} km/h` : null,
    ]
      .filter(Boolean)
      .join(", ");
    return h(
      "li.hour",
      { class: firstOfDay ? "hour day-start" : "hour" },
      h("span.visually-hidden", null, spoken),
      h("span.hour-day", { "aria-hidden": "true" }, firstOfDay ? dayName(date, today) : ""),
      h("span.hour-time", { "aria-hidden": "true" }, index === 0 ? "Ahora" : hourLabel(hour.time, timeZone)),
      h("span.hour-icon", { "aria-hidden": "true" }, hour.sky ? icon(condition.icon, "icon icon-condition") : null),
      h("span.hour-reading", { "aria-hidden": "true" }, readings(hour, levels[index])),
    );
  });

  // Temperature curve through the dots, with a soft fill below it.
  const points = levels.map((level, index) =>
    level === null ? null : /** @type {[number, number]} */ ([index + 0.5, dotPosition(level)]),
  );
  const line = smoothPath(points);
  const first = points.find(Boolean);
  const last = [...points].reverse().find(Boolean);
  const curve = svg(
    "svg",
    {
      class: "temperature-curve",
      viewBox: `0 0 ${hours.length} 100`,
      preserveAspectRatio: "none",
      "aria-hidden": "true",
    },
    svg(
      "defs",
      null,
      svg(
        "linearGradient",
        { id: "curve-fade", x1: "0", x2: "0", y1: "0", y2: "1" },
        svg("stop", { offset: "0", class: "fade-start" }),
        svg("stop", { offset: "1", class: "fade-end" }),
      ),
    ),
    first && last && points.every(Boolean)
      ? svg("path", {
          class: "curve-fill",
          d: `${line} L${last[0]},100 L${first[0]},100 Z`,
          fill: "url(#curve-fade)",
        })
      : null,
    svg("path", { class: "curve-line", d: line }),
  );

  const scroller = h(
    "div.hours-scroller",
    {
      tabindex: "0",
      role: "region",
      "aria-label": `${current.spoken} por horas, desplazable`,
    },
    h("div.hours-track", { style: { "--hours": hours.length } }, curve, h("ol.hours", { role: "list" }, items)),
  );
  scrollerElement = scroller;
  // The edges fade where there are more hours to scroll to.
  const markEnd = () => {
    scroller.classList.toggle("scrolled", scroller.scrollLeft > 4);
    scroller.classList.toggle("at-end", scroller.scrollLeft + scroller.clientWidth >= scroller.scrollWidth - 4);
  };
  scroller.addEventListener("scroll", markEnd, { passive: true });
  requestAnimationFrame(() => {
    scroller.scrollLeft = scrollLeft;
    markEnd();
  });

  card = h(
    "section.card.hourly",
    {
      class: `card hourly mode-${current.value}`,
      "aria-labelledby": "hourly-title",
    },
    header,
    scroller,
    caption,
  );
  return card;
}
