/**
 * Forecast of one beach: morning and afternoon for three days.
 */

import { beachSky, beachWaves, beachWind, sky, uvCategory } from "./codes.js";
import { offlineNote, staleNote, temperatureRange, warningsBanner } from "./cards.js";
import { h, icon, identity, infoButton, preserveFocus, replaceChildren } from "./dom.js";
import { dayMonth, dayName, degrees, localDate, localHour } from "./format.js";
import { currentHour, daySummary } from "./forecast.js";
import { PlaceHeader } from "./header.js";
import { RENDER_EVERY, warningsKey } from "./view-forecast.js";

/** @typedef {import("./places.js").Beach} Beach */
/** @typedef {import("./forecast.js").Forecast} Forecast */
/** @typedef {import("./warnings.js").Warning} Warning */

/**
 * @typedef {object} BeachState
 * @property {string} id
 * @property {Beach | null} beach
 * @property {any} forecast Beach document from `beaches/<PP>.json`.
 * @property {Forecast | null} municipality Forecast of the beach's municipality.
 * @property {Warning[]} warnings
 * @property {boolean} saved
 * @property {string | null} status
 * @property {boolean} offline
 * @property {number} checked When the data was last confirmed from the network (ms), or 0.
 * @property {number} now
 */

/**
 * A morning or afternoon column.
 *
 * @param {string} label
 * @param {any} day
 * @param {number} half 0 morning, 1 afternoon.
 */
function half(label, day, half) {
  const condition = beachSky(day.sky?.[half]);
  const wind = beachWind(day.wind?.[half]);
  const waves = beachWaves(day.waves?.[half]);
  return h(
    "div.beach-half",
    null,
    h("h4.beach-half-label", null, label),
    h("span.beach-half-icon", null, icon(condition.icon, "icon icon-large")),
    h("p.beach-half-sky", null, condition.label),
    h(
      "ul.beach-levels",
      { role: "list" },
      h(
        "li",
        { class: `level-${wind?.level ?? 0}` },
        icon("wind", "icon icon-small"),
        h("span.level-name", null, "Viento "),
        h("strong", null, wind?.label ?? "—"),
      ),
      h(
        "li",
        { class: `level-${waves?.level ?? 0}` },
        icon("waves", "icon icon-small"),
        h("span.level-name", null, "Oleaje "),
        h("strong", null, waves?.label ?? "—"),
      ),
    ),
  );
}

/**
 * One beach day. Air temperature and rain come from the municipality's
 * forecast for that day; AEMET's beach forecast gives the rest.
 *
 * @param {any} day
 * @param {import("./forecast.js").Day | null} town The municipality's forecast for the day.
 * @param {{today: string, hour: number}} now
 * @param {boolean} first
 */
function beachDay(day, town, { today, hour }, first) {
  const uv = uvCategory(day.uv);
  const titleId = `beach-day-${day.date}`;
  const low = town?.tmin ?? null;
  const high = town?.tmax ?? day.tmax ?? null;
  const pop = town ? daySummary(town, { today, hour: day.date === today ? hour : -1 }).pop : null;
  /** @type {Array<{icon: string, label: string, value: unknown, className: string} | null>} */
  const facts = [
    day.water != null
      ? {
          icon: "waves",
          label: "Agua",
          value: degrees(day.water),
          className: "water",
        }
      : null,
    high !== null
      ? {
          icon: "thermometer",
          label: "Temperatura",
          value: low !== null ? temperatureRange(low, high) : [h("small", null, "Máx "), degrees(high)],
          className: "air",
        }
      : null,
    pop !== null
      ? {
          icon: "umbrella",
          label: "Lluvia",
          value: `${pop}%`,
          className: "rain",
        }
      : null,
    uv
      ? {
          icon: "sun-dim",
          label: "Índice UV",
          value: [String(day.uv), h("small", null, ` ${uv.label.toLowerCase()}`)],
          className: `uv uv-${uv.level}`,
        }
      : null,
  ];
  return h(
    "section.card.beach-day",
    {
      class: first ? "card beach-day first" : "card beach-day",
      "aria-labelledby": titleId,
    },
    h(
      "div.card-header",
      null,
      h(
        "h2.card-title",
        { id: titleId },
        dayName(day.date, today),
        h("span.card-title-detail", null, dayMonth(day.date)),
        first ? infoButton("playas", "previsión de playas") : null,
      ),
    ),
    h(
      "ul.beach-facts",
      { role: "list" },
      facts.map((fact) =>
        !fact
          ? null
          : h(
              "li",
              { class: fact.className || null },
              h("span.fact-label", null, icon(fact.icon, "icon icon-small"), fact.label),
              h("strong.fact-value", null, fact.value),
            ),
      ),
    ),
    h("div.beach-halves", null, half("Mañana", day, 0), half("Tarde", day, 1)),
  );
}

export class BeachView {
  /**
   * @param {HTMLElement} root
   * @param {object} callbacks
   * @param {() => void} callbacks.onBack
   * @param {() => void} callbacks.onSave Save this beach.
   */
  constructor(root, { onBack, onSave }) {
    this.header = new PlaceHeader(
      {
        onBack,
        onSave: () => {
          onSave();
          this.header.confirmSaved();
          this.header.focusTitle();
        },
      },
      "beach-title",
    );
    this.content = h("div.place-content.beach-content");
    /** What the cards were last built from. */
    this.renderedKey = "";
    replaceChildren(root, this.header.element, this.content);
  }

  focusTitle() {
    this.header.focusTitle();
  }

  /** @param {BeachState} state */
  render({ id, beach, forecast, municipality, warnings, saved, status, offline, checked, now }) {
    this.header.update({
      title: beach?.name ?? forecast?.name ?? "Playa",
      subtitle: beach
        ? `Playa en ${beach.municipality}${beach.province && beach.province !== beach.municipality ? `, ${beach.province}` : ""}`
        : "Playa",
      saved,
      known: beach !== null,
      stale: forecast ? staleNote(checked, now) : null,
    });
    const timeZone = beach?.timeZone ?? "Europe/Madrid";
    const today = localDate(now, timeZone);
    const clock = { today, hour: localHour(now, timeZone) };
    const townDay = (/** @type {string} */ date) =>
      municipality?.days.find((candidate) => candidate.date === date) ?? null;
    // Days AEMET sent without any value are left out.
    const hasData = (/** @type {any} */ day) =>
      [...(day.sky ?? []), ...(day.wind ?? []), ...(day.waves ?? []), day.tmax, day.water, day.uv, day.feel].some(
        (value) => value != null,
      );
    const days = (forecast?.days ?? []).filter((/** @type {any} */ day) => day.date >= today && hasData(day));
    // Rebuilt only when something shown changed (see the forecast view).
    const hour = municipality ? currentHour(municipality, now) : null;
    const key = [
      id,
      beach?.name ?? "",
      identity(forecast?.days),
      identity(municipality),
      warningsKey(warnings),
      status ?? "",
      offline,
      hour?.time ?? "",
      today,
      Math.floor(now / RENDER_EVERY),
    ].join("|");
    if (key === this.renderedKey) return;
    this.renderedKey = key;
    if (days.length === 0) {
      replaceChildren(
        this.content,
        h(
          "section.card.now.now-empty",
          { "aria-live": "polite" },
          h("p", null, status ?? "No hay previsión para esta playa."),
        ),
      );
      return;
    }

    // Current conditions in the municipality, as a link to its forecast.
    const condition = sky(hour?.sky);
    const municipalityLink = beach
      ? h(
          "a.card.link-card",
          { href: `#/m/${beach.municipalityId}` },
          h("span.link-card-icon", null, icon(hour ? condition.icon : "clock", "icon icon-condition")),
          h(
            "span.link-text",
            null,
            h("span.link-name", null, `Por horas en ${beach.municipality}`),
            h(
              "span.link-detail",
              null,
              hour
                ? `Ahora ${degrees(hour.temp)}, ${condition.label.toLowerCase()}`
                : "Temperatura, lluvia y viento hora a hora",
            ),
          ),
          icon("chevron-right", "icon icon-small link-chevron"),
        )
      : null;

    preserveFocus(this.content, () =>
      replaceChildren(
        this.content,
        warningsBanner(warnings, timeZone, now, `#/p/${id}/avisos`),
        h(
          "div.cards.beach-cards",
          null,
          h(
            "div.column",
            null,
            days.slice(0, 1).map((day) => beachDay(day, townDay(day.date), clock, true)),
            municipalityLink,
          ),
          h(
            "div.column",
            null,
            days.slice(1).map((day) => beachDay(day, townDay(day.date), clock, false)),
          ),
        ),
        offlineNote(offline),
      ),
    );
  }
}
