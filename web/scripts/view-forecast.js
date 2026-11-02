/**
 * Forecast of one municipality: cards on a scrolling page.
 */

import { beachLinksCard, dailyCard, detailsCard, nowCard, offlineNote, staleNote, warningsBanner } from "./cards.js";
import { hourlyCard } from "./card-hourly.js";
import { h, identity, preserveFocus, replaceChildren } from "./dom.js";
import { localDate, localHour } from "./format.js";
import { currentHour, hoursSummary, upcomingDays, upcomingHours } from "./forecast.js";
import { PlaceHeader } from "./header.js";

/** Longest time the cards go without being rebuilt (ms). */
export const RENDER_EVERY = 10 * 60 * 1000;

/**
 * Everything shown of the warnings, for a view's render key: a warning can
 * change level or timing and keep its id.
 *
 * @param {Warning[]} warnings
 */
export function warningsKey(warnings) {
  return warnings.map((warning) => `${warning.id}:${warning.level}:${warning.onset}:${warning.expires}`).join();
}

/** @typedef {import("./forecast.js").Forecast} Forecast */
/** @typedef {import("./places.js").Place} Place */
/** @typedef {import("./warnings.js").Warning} Warning */

/**
 * @typedef {object} ForecastState
 * @property {string} id
 * @property {Place | null} place
 * @property {Forecast | null} forecast
 * @property {Warning[]} warnings
 * @property {any[]} beaches Beach forecasts of the municipality.
 * @property {boolean} saved
 * @property {string | null} status Message when the forecast is missing.
 * @property {boolean} offline
 * @property {number} checked When the data was last confirmed from the network (ms), or 0.
 * @property {number} now
 */

export class ForecastView {
  /**
   * @param {HTMLElement} root
   * @param {object} callbacks
   * @param {() => void} callbacks.onBack
   * @param {() => void} callbacks.onSave Save this municipality.
   * @param {(mode: string) => void} callbacks.onModeChange
   * @param {string} mode Initial hourly mode.
   */
  constructor(root, { onBack, onSave, onModeChange }, mode) {
    this.mode = mode;
    this.onModeChange = onModeChange;
    /** @type {string | null} */
    this.renderedId = null;
    /** @type {ForecastState | null} */
    this.state = null;
    /** What the cards were last built from. */
    this.renderedKey = "";

    this.header = new PlaceHeader(
      {
        onBack,
        onSave: () => {
          onSave();
          this.header.confirmSaved();
          this.header.focusTitle();
        },
      },
      "forecast-title",
    );
    this.content = h("div.place-content");
    replaceChildren(root, this.header.element, this.content);
  }

  focusTitle() {
    this.header.focusTitle();
  }

  /** @param {ForecastState} state */
  render(state) {
    this.state = state;
    const { id, place, forecast, warnings, beaches, saved, status, offline, checked, now } = state;
    const scroller = this.content.querySelector(".hours-scroller");
    const scrollLeft = id === this.renderedId && scroller ? scroller.scrollLeft : 0;
    this.renderedId = id;

    // The province is left out when it has the same name as the municipality.
    const province = place && place.province !== place.name ? place.province : "";
    this.header.update({
      title: place?.name ?? "Municipio",
      subtitle: province,
      saved,
      known: place !== null,
      stale: forecast ? staleNote(checked, now) : null,
    });

    if (!forecast) {
      const emptyKey = `${id}|empty|${status}`;
      if (this.renderedKey === emptyKey) return;
      this.renderedKey = emptyKey;
      replaceChildren(this.content, nowCard({ hour: null, today: null, summary: [], status }));
      return;
    }

    const timeZone = forecast.timeZone;
    const today = localDate(now, timeZone);
    const hour = localHour(now, timeZone);
    const current = currentHour(forecast, now);
    // Rebuilding the cards replaces what is under the finger and can move
    // the page, so it only happens when something shown changed: the data,
    // the hour, or (for the sun's progress) every 10 minutes.
    const key = [
      id,
      identity(forecast),
      warningsKey(warnings),
      beaches.map((beach) => `${beach.id}:${beach.name}:${beach.issued ?? ""}:${identity(beach.days)}`).join(),
      place?.name ?? "",
      status ?? "",
      offline,
      current?.time ?? "",
      today,
      Math.floor(now / RENDER_EVERY),
    ].join("|");
    if (key === this.renderedKey) return;
    this.renderedKey = key;
    const next = upcomingHours(forecast, now);
    const days = upcomingDays(forecast, now);
    const todayForecast = days.find((day) => day.date === today) ?? null;
    const base = `#/m/${id}`;
    preserveFocus(this.content, () =>
      replaceChildren(
        this.content,
        warningsBanner(warnings, timeZone, now, `${base}/avisos`),
        h(
          "div.cards",
          null,
          h(
            "div.column",
            null,
            nowCard({
              hour: current,
              today: todayForecast,
              summary: hoursSummary(next, timeZone, now),
              status: status ?? "Sin previsión para esta hora",
            }),
            hourlyCard({
              hours: next,
              timeZone,
              now,
              mode: this.mode,
              scrollLeft,
              onModeChange: (mode) => {
                this.mode = mode;
                this.onModeChange(mode);
              },
            }),
            dailyCard(days, {
              today,
              hour,
              timeZone,
              warnings,
              hrefFor: (date) => `${base}/d/${date}`,
            }),
          ),
          h(
            "div.column",
            null,
            detailsCard(forecast, current, todayForecast, now),
            beachLinksCard(beaches, today, hour, place?.name ?? ""),
          ),
        ),
        offlineNote(offline),
      ),
    );
  }
}
