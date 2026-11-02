/**
 * Application controller: routing, data loading and rendering.
 */

import { beachSky, sky } from "./codes.js";
import { DataStore } from "./data-store.js";
import { replaceChildren, swipeToClose } from "./dom.js";
import { checkedAt, localDate, localHour } from "./format.js";
import { currentHour, decodeForecast } from "./forecast.js";
import {
  PLACES_PATH,
  WARNINGS_PATH,
  beachesPath,
  decodeBeaches,
  decodePlaces,
  forecastPath,
  isBeachId,
  timeZoneFor,
} from "./places.js";
import * as preferences from "./preferences.js";
import { formatRoute, parseRoute, placeHref } from "./router.js";
import { Guide } from "./guide.js";
import { daySheet, warningsSheet } from "./sheets.js";
import { BeachView } from "./view-beach.js";
import { ForecastView } from "./view-forecast.js";
import { LocationsView } from "./view-locations.js";
import { highestLevel, warningsFor } from "./warnings.js";

const MINUTE = 60 * 1000;
const FORECAST_MAX_AGE = 10 * MINUTE;
const PLACES_MAX_AGE = 6 * 60 * MINUTE;
const EXTRAS_MAX_AGE = 60 * MINUTE;
/** Days of a beach whose forecast is not loaded; one array, so it never looks like new data. */
const NO_DAYS = Object.freeze([]);
/** A relaunch within this time opens where the page was left. */
const SCROLL_MEMORY = 30 * MINUTE;

/** @typedef {import("./router.js").Route} Route */

export class App {
  /** @param {HTMLElement} root */
  constructor(root) {
    const dataUrl = new URL(root.dataset.weatherDataUrl ?? "../data/", location.href).href;
    this.store = new DataStore(dataUrl);
    /** @type {Map<string, import("./places.js").Place>} */
    this.places = new Map();
    /** @type {Map<string, import("./places.js").Beach>} */
    this.beaches = new Map();
    /** @type {unknown} */
    this.placesDocument = null;
    this.saved = preferences.savedIds();
    /** @type {Route} */
    this.route = parseRoute(location.hash);
    /** Steps from the current screen back to the list. */
    this.depth = 0;
    /** Scroll position of each screen, by its route. @type {Map<string, number>} */
    this.scrollPositions = new Map();
    this.renderQueued = false;
    /** A finger or button is down (since when, ms), or 0. */
    this.pointerDownAt = 0;
    /** Background renders wait until this time (performance.now()). */
    this.quietUntil = 0;
    /** Scroll position to restore once the page is tall enough, after a relaunch. @type {number | null} */
    this.pendingScroll = null;
    this.sheetKey = "";
    /** Page scroll when the sheet opened, kept while it is open. */
    this.sheetPageScroll = 0;
    this.sheetScreen = "";
    /** @type {WeakMap<object, Map<string, ReturnType<typeof decodeForecast>>>} */
    this.decoded = new WeakMap();

    this.locationsSection = /** @type {HTMLElement} */ (root.querySelector("#locations-view"));
    this.forecastSection = /** @type {HTMLElement} */ (root.querySelector("#forecast-view"));
    this.beachSection = /** @type {HTMLElement} */ (root.querySelector("#beach-view"));
    this.dialog = /** @type {HTMLDialogElement} */ (root.querySelector("#sheet"));
    this.guide = new Guide(/** @type {HTMLDialogElement} */ (root.querySelector("#guide")));

    this.locations = new LocationsView(this.locationsSection, {
      onReorder: (ids) => this.setSaved(ids),
      onRemove: (id) => this.setSaved(this.saved.filter((saved) => saved !== id)),
      onPrefetch: (id) => this.loadPlace(id),
    });
    // Saving adds the open place at the end of the list.
    const save = () => {
      if (this.route.view !== "locations") this.setSaved([...this.saved, this.route.id]);
    };
    const back = () => this.goBack();
    this.forecastView = new ForecastView(
      this.forecastSection,
      {
        onBack: back,
        onSave: save,
        onModeChange: (mode) => preferences.setHourlyMode(mode),
      },
      preferences.hourlyMode(),
    );
    this.beachView = new BeachView(this.beachSection, {
      onBack: back,
      onSave: save,
    });

    this.dialog.addEventListener("close", () => {
      // Closing returns focus to the page, which some browsers scroll to.
      // Only on the screen the sheet was opened from (closing it by going to
      // another screen must not pull that screen's scroll back).
      const pageScroll = this.sheetPageScroll;
      const screen = this.sheetScreen;
      const keep = () => {
        if (screenOf(this.route) === screen && window.scrollY !== pageScroll) window.scrollTo(0, pageScroll);
      };
      keep();
      requestAnimationFrame(keep);
      if (this.route.view !== "locations" && this.route.sheet) {
        this.closeSheet();
      }
    });
    swipeToClose(this.dialog);
    this.dialog.addEventListener("click", (event) => {
      // A click on the backdrop closes the sheet.
      if (event.target === this.dialog) this.dialog.close();
    });
  }

  async start() {
    // The app decides where the page scrolls: links inside the app never
    // jump, and going back returns to where you were.
    history.scrollRestoration = "manual";
    window.addEventListener("hashchange", () => {
      const route = parseRoute(location.hash);
      // An entry the app did not create (a URL edited by hand) gets its
      // distance from the list, one more than the screen it came from.
      if (typeof (/** @type {{depth?: unknown} | null} */ (history.state)?.depth) !== "number") {
        history.replaceState({ depth: route.view === "locations" ? 0 : this.depth + 1 }, "", location.hash);
      }
      this.applyRoute(route, { restoreScroll: true });
    });
    // Any ⓘ button opens the guide at its section.
    document.addEventListener("click", (event) => {
      const button = event.target instanceof Element ? event.target.closest("[data-guide]") : null;
      if (button) this.guide.open(button.getAttribute("data-guide") || undefined);
    });
    document.addEventListener("click", (event) => {
      const link = event.target instanceof Element ? event.target.closest("a[href^='#/']") : null;
      if (
        !link ||
        event.defaultPrevented ||
        event.button !== 0 ||
        event.metaKey ||
        event.ctrlKey ||
        event.shiftKey ||
        event.altKey
      )
        return;
      event.preventDefault();
      // Navigate after the tap is fully handled, so replacing the tapped
      // link cannot make the browser scroll.
      const route = parseRoute(link.getAttribute("href") ?? "#/");
      requestAnimationFrame(() => this.navigate(route));
    });
    // Background updates wait while the user touches or scrolls the page:
    // rebuilding the element under the finger loses the tap, and changing
    // the page during a scroll can move it.
    const quiet = (/** @type {number} */ ms) => {
      this.quietUntil = Math.max(this.quietUntil, performance.now() + ms);
    };
    const options = { capture: true, passive: true };
    document.addEventListener(
      "pointerdown",
      () => {
        this.pointerDownAt = performance.now();
        this.pendingScroll = null;
      },
      options,
    );
    const release = () => {
      this.pointerDownAt = 0;
      quiet(500);
    };
    document.addEventListener("pointerup", release, options);
    document.addEventListener("pointercancel", release, options);
    // Any scroller, including the hours strip (scroll events do not bubble).
    document.addEventListener("scroll", () => quiet(300), options);
    // Scrolling by wheel or keys also means the user has taken over.
    const takeOver = () => {
      this.pendingScroll = null;
    };
    window.addEventListener("wheel", takeOver, { passive: true });
    window.addEventListener("keydown", takeOver);
    // Lets iOS show the pressed (:active) state of buttons and rows.
    document.addEventListener("touchstart", () => {}, { passive: true });

    // Remember where the page is, for a relaunch.
    let saveTimer = 0;
    const saveScroll = () => {
      clearTimeout(saveTimer);
      saveTimer = 0;
      preferences.setScrollMemory(screenOf(this.route), window.scrollY);
    };
    window.addEventListener(
      "scroll",
      () => {
        if (!saveTimer) saveTimer = setTimeout(saveScroll, 400);
      },
      { passive: true },
    );
    window.addEventListener("pagehide", saveScroll);

    this.store.addEventListener("change", () => this.scheduleRender());
    window.addEventListener("online", () => this.store.refreshAll(0));
    window.addEventListener("offline", () => this.scheduleRender());
    document.addEventListener("visibilitychange", () => {
      if (document.visibilityState === "hidden") saveScroll();
      if (document.visibilityState === "visible") {
        this.store.refreshAll(FORECAST_MAX_AGE);
        this.scheduleRender();
      }
    });
    // Keep "now" current and refresh data while the app stays open.
    setInterval(() => {
      if (document.visibilityState === "visible") this.scheduleRender();
    }, MINUTE);
    setInterval(() => {
      if (document.visibilityState === "visible") this.store.refreshAll(FORECAST_MAX_AGE);
    }, FORECAST_MAX_AGE);

    // Every history entry records how many steps it is from the list
    // (`depth`), so the back arrow knows whether there is a screen of the
    // app to go back to. The list is always the first entry: on launch the
    // last opened place, or a place opened from a link, goes on top of it.
    let launch = location.hash;
    if (!launch || launch === "#" || launch === "#/") {
      const last = preferences.lastOpenedId();
      launch = last && this.saved.includes(last) ? placeHref(last) : "#/";
    }
    history.replaceState({ depth: 0 }, "", "#/");
    if (launch !== "#/") history.pushState({ depth: 1 }, "", launch);
    const launchRoute = parseRoute(location.hash);
    this.applyRoute(launchRoute);
    // After the launch screen is shown (showing it starts at the top), go
    // back to where it was left, once its content is tall enough.
    const memory = preferences.scrollMemory();
    if (memory && memory.y > 0 && Date.now() - memory.at < SCROLL_MEMORY && memory.screen === screenOf(launchRoute)) {
      this.pendingScroll = memory.y;
      this.restoreScroll();
    }
    void this.store.load(PLACES_PATH, { maxAge: PLACES_MAX_AGE });
    void this.store.load(WARNINGS_PATH, { maxAge: FORECAST_MAX_AGE });
    for (const id of this.saved) this.loadPlace(id);
  }

  /**
   * Load the data a place's screen and its saved row need.
   *
   * @param {string} id
   */
  loadPlace(id) {
    if (isBeachId(id)) {
      const municipality = this.beaches.get(id)?.municipalityId ?? id.slice(0, 5);
      void this.store.load(beachesPath(municipality), {
        maxAge: EXTRAS_MAX_AGE,
      });
      void this.store.load(forecastPath(municipality), {
        maxAge: FORECAST_MAX_AGE,
      });
    } else {
      void this.store.load(forecastPath(id), { maxAge: FORECAST_MAX_AGE });
    }
  }

  /** @param {Route} route */
  navigate(route) {
    const hash = formatRoute(route);
    if (hash !== location.hash) {
      const depth = route.view === "locations" ? 0 : historyDepth() + 1;
      // A sheet opened over its own place closes by going back one step.
      const current = this.route;
      const overPlace =
        route.view !== "locations" && route.sheet !== null && current.view === route.view && current.id === route.id;
      // The entry being left keeps its scroll position, so going back to it
      // returns to the same place even if the screen is visited again later.
      history.replaceState({ ...history.state, scroll: Math.round(window.scrollY) }, "", location.hash);
      history.pushState({ depth, overPlace }, "", hash);
    }
    this.applyRoute(route);
  }

  /**
   * The back arrow: to the previous screen, like the system's back. Sheets
   * and the guide remove their own entries when they close, so the entry
   * before this one is always a screen.
   */
  goBack() {
    if (historyDepth() > 0) {
      history.back();
    } else {
      // An entry without depth (a URL typed by hand): replace it with the list.
      history.replaceState({ depth: 0 }, "", "#/");
      this.applyRoute({ view: "locations" }, { restoreScroll: true });
    }
  }

  closeSheet() {
    if (this.route.view === "locations") return;
    if (/** @type {{overPlace?: boolean} | null} */ (history.state)?.overPlace) {
      history.back();
    } else {
      // Opened from a link: show the place under it in the same entry.
      history.replaceState({ depth: historyDepth() }, "", screenOf(this.route));
      this.applyRoute(parseRoute(location.hash));
    }
  }

  /**
   * @param {Route} route
   * @param {{restoreScroll?: boolean}} [options] Restore the remembered
   *   scroll position (going back or forward), instead of starting at the top.
   */
  applyRoute(route, { restoreScroll = false } = {}) {
    const previous = this.route;
    const changedView = screenOf(previous) !== screenOf(route);
    if (changedView) {
      this.scrollPositions.set(screenOf(previous), window.scrollY);
      this.pendingScroll = null;
    }
    this.route = route;
    this.depth = historyDepth();
    this.locationsSection.hidden = route.view !== "locations";
    this.forecastSection.hidden = route.view !== "forecast";
    this.beachSection.hidden = route.view !== "beach";

    if (route.view === "locations") {
      preferences.setLastOpenedId(null);
    } else {
      preferences.setLastOpenedId(this.saved.includes(route.id) ? route.id : preferences.lastOpenedId());
      this.loadPlace(route.id);
      void this.store.load(WARNINGS_PATH, { maxAge: FORECAST_MAX_AGE });
    }
    this.render();

    // A new screen starts at the top (or where it was left, going back or to
    // the list) and moves focus to its heading. Sheets never move the page.
    if (changedView) {
      const entry = /** @type {{scroll?: unknown} | null} */ (history.state)?.scroll;
      const remembered =
        restoreScroll && typeof entry === "number" ? entry : this.scrollPositions.get(screenOf(route));
      window.scrollTo(0, (restoreScroll || route.view === "locations") && remembered !== undefined ? remembered : 0);
      if (route.view === "forecast") this.forecastView.focusTitle();
      else if (route.view === "beach") this.beachView.focusTitle();
      else if (previous.view !== "locations") {
        const row = this.locationsSection.querySelector(`a[href="${placeHref(previous.id)}"]`);
        /** @type {HTMLElement | null} */ (row)?.focus({ preventScroll: true });
      }
    }
  }

  /** @param {string[]} ids */
  setSaved(ids) {
    this.saved = [...new Set(ids)];
    preferences.setSavedIds(this.saved);
    for (const id of this.saved) this.loadPlace(id);
    this.render();
  }

  /** Render on the next frame, once the user is not touching or scrolling. */
  scheduleRender() {
    if (this.renderQueued) return;
    this.renderQueued = true;
    const attempt = () => {
      const now = performance.now();
      // A press that never ended (a lost pointerup) stops counting after 3 s.
      const pressed = this.pointerDownAt > 0 && now - this.pointerDownAt < 3000;
      const wait = pressed ? 150 : this.quietUntil - now;
      if (wait > 0) {
        setTimeout(attempt, wait);
        return;
      }
      requestAnimationFrame(() => {
        if (this.pointerDownAt > 0 && performance.now() - this.pointerDownAt < 3000) {
          attempt();
          return;
        }
        this.renderQueued = false;
        this.render();
      });
    };
    attempt();
  }

  /** After a relaunch, go back to where the page was once it is tall enough. */
  restoreScroll() {
    if (this.pendingScroll === null) return;
    const room = document.documentElement.scrollHeight - window.innerHeight;
    if (room >= this.pendingScroll - 1) {
      window.scrollTo(0, this.pendingScroll);
      this.pendingScroll = null;
    }
  }

  /**
   * Decoded forecast of a municipality, if its bundle is loaded. It does not
   * wait for the place list: the time zone comes from the province.
   *
   * @param {string} id
   */
  forecast(id) {
    const bundle = /** @type {any} */ (this.store.peek(forecastPath(id)).data);
    if (!bundle) return null;
    let cache = this.decoded.get(bundle);
    if (!cache) {
      cache = new Map();
      this.decoded.set(bundle, cache);
    }
    if (!cache.has(id)) {
      cache.set(id, decodeForecast(id, bundle.municipalities?.[id], timeZoneFor(id)));
    }
    return cache.get(id) ?? null;
  }

  /**
   * Forecast document of a beach, if loaded.
   *
   * @param {string} id
   */
  beachForecast(id) {
    const document = /** @type {any} */ (this.store.peek(beachesPath(id)).data);
    const beach = document?.beaches?.[id];
    return beach ? { ...beach, id, issued: beach.issued ?? document.issued } : null;
  }

  /**
   * Beaches of a municipality with their forecasts, in list order.
   *
   * @param {string} id
   */
  municipalityBeaches(id) {
    return [...this.beaches.values()]
      .filter((beach) => beach.municipalityId === id)
      .map((beach) => ({
        name: beach.name,
        ...(this.beachForecast(beach.id) ?? { id: beach.id, days: NO_DAYS }),
      }));
  }

  /**
   * Warnings at a place: a beach takes its municipality's.
   *
   * @param {string} municipalityId
   * @param {number} now
   */
  warnings(municipalityId, now) {
    const forecast = this.forecast(municipalityId);
    return warningsFor(this.store.peek(WARNINGS_PATH).data, municipalityId, forecast?.meta.zone, now);
  }

  render() {
    this.renderScreen();
    this.restoreScroll();
  }

  renderScreen() {
    const now = Date.now();
    const placesEntry = this.store.peek(PLACES_PATH);
    if (placesEntry.data && placesEntry.data !== this.placesDocument) {
      this.placesDocument = placesEntry.data;
      this.places = decodePlaces(placesEntry.data);
      this.beaches = decodeBeaches(placesEntry.data, this.places);
    }
    const offline = navigator.onLine === false;

    if (this.route.view === "locations") {
      this.renderLocations(now, placesEntry, offline);
      if (this.dialog.open) this.dialog.close();
      document.title = "Cielo";
      return;
    }

    const { id } = this.route;
    const saved = this.saved.includes(id);
    if (this.route.view === "beach") {
      const beach = this.beaches.get(id) ?? null;
      const forecast = this.beachForecast(id);
      const municipalityId = beach?.municipalityId ?? id.slice(0, 5);
      const entry = this.store.peek(beachesPath(municipalityId));
      const warnings = this.warnings(municipalityId, now);
      this.beachView.render({
        id,
        beach,
        forecast,
        municipality: this.forecast(municipalityId),
        warnings,
        saved,
        status: this.statusFor(
          entry,
          forecast !== null,
          beach !== null || this.beaches.size === 0,
          offline,
          "esta playa",
        ),
        offline,
        checked: entry.fetchedAt,
        now,
      });
      document.title = `${beach?.name ?? "Playa"} · Cielo`;
      this.renderSheet(null, warnings, beach?.timeZone ?? timeZoneFor(id), now);
      return;
    }

    const place = this.places.get(id) ?? null;
    const forecast = this.forecast(id);
    const warnings = this.warnings(id, now);
    const beaches = this.municipalityBeaches(id);
    if (beaches.length > 0) {
      void this.store.load(beachesPath(id), { maxAge: EXTRAS_MAX_AGE });
    }
    this.forecastView.render({
      id,
      place,
      forecast,
      warnings,
      beaches,
      saved,
      status: this.statusFor(
        this.store.peek(forecastPath(id)),
        forecast !== null,
        place !== null || this.places.size === 0,
        offline,
        "este municipio",
      ),
      offline,
      checked: this.store.peek(forecastPath(id)).fetchedAt,
      now,
    });
    document.title = place ? `${place.name} · Cielo` : "Cielo";
    this.renderSheet(forecast, warnings, timeZoneFor(id), now);
  }

  /**
   * Message for a screen whose data is missing.
   *
   * @param {import("./data-store.js").Entry} entry
   * @param {boolean} loaded
   * @param {boolean} known
   * @param {boolean} offline
   * @param {string} what
   */
  statusFor(entry, loaded, known, offline, what) {
    if (loaded) return null;
    if (!known) return `No hay previsión para ${what}.`;
    if (entry.status === "offline" || offline) return "Sin conexión. La previsión se cargará al volver a conectarte.";
    if (entry.status === "missing") return `No hay previsión para ${what} ahora mismo.`;
    if (entry.status === "error") return "No se pudo cargar la previsión. Inténtalo de nuevo en un momento.";
    if (entry.status === "ready") return `No hay previsión para ${what}.`;
    return "Cargando previsión…";
  }

  /**
   * @param {number} now
   * @param {import("./data-store.js").Entry} placesEntry
   * @param {boolean} offline
   */
  renderLocations(now, placesEntry, offline) {
    /** @type {Map<string, import("./view-locations.js").SavedSummary>} */
    const summaries = new Map();
    for (const id of this.saved) {
      if (isBeachId(id)) {
        const beach = this.beachForecast(id);
        const timeZone = this.beaches.get(id)?.timeZone ?? timeZoneFor(id);
        const today = localDate(now, timeZone);
        const day = beach?.days?.find((/** @type {any} */ candidate) => candidate.date === today) ?? null;
        const condition = beachSky(day?.sky?.[localHour(now, timeZone) < 14 ? 0 : 1]);
        summaries.set(id, {
          temp: day?.water ?? null,
          icon: day ? condition.icon : null,
          condition: condition.label,
          warning: highestLevel(this.warnings(this.beaches.get(id)?.municipalityId ?? id.slice(0, 5), now)),
        });
        continue;
      }
      const forecast = this.forecast(id);
      const hour = forecast ? currentHour(forecast, now) : null;
      const condition = sky(hour?.sky);
      summaries.set(id, {
        temp: hour?.temp ?? null,
        icon: hour?.sky ? condition.icon : null,
        condition: condition.label,
        warning: highestLevel(this.warnings(id, now)),
      });
    }
    let status = null;
    if (this.places.size === 0) {
      status =
        placesEntry.status === "error" || placesEntry.status === "offline" || placesEntry.status === "missing"
          ? offline
            ? "Sin conexión. Conéctate a Internet para buscar lugares."
            : "No se pudo cargar la lista de lugares. Inténtalo de nuevo más tarde."
          : "Cargando lugares…";
    }
    // The last time the data was checked, even if AEMET had nothing new.
    const checked = this.store.lastChecked();
    this.locations.update({
      places: this.places,
      beaches: this.beaches,
      saved: this.saved,
      summaries,
      status,
      updated: checked ? `${checkedAt(checked, now)}${offline ? " · sin conexión" : ""}` : null,
    });
  }

  /**
   * @param {import("./forecast.js").Forecast | null} forecast
   * @param {import("./warnings.js").Warning[]} warnings
   * @param {string} timeZone
   * @param {number} now
   */
  renderSheet(forecast, warnings, timeZone, now) {
    const route = this.route;
    const sheet = route.view === "locations" ? null : route.sheet;
    if (!sheet || (sheet.kind === "day" && !forecast)) {
      if (this.dialog.open) this.dialog.close();
      return;
    }
    // Only rebuild an open sheet when its content changed, to keep focus and position.
    const key = `${formatRoute(route)}|${forecast?.issued?.getTime()}|${warnings.map((warning) => warning.id).join(",")}`;
    if (this.dialog.open && key === this.sheetKey) return;
    this.sheetKey = key;
    const close = () => this.dialog.close();
    const body = this.dialog.querySelector(".sheet-body");
    const scrollTop = body?.scrollTop ?? 0;
    replaceChildren(
      this.dialog,
      sheet.kind === "day" && forecast
        ? daySheet(
            forecast,
            sheet.date,
            now,
            close,
            warnings,
            formatRoute({
              view: "forecast",
              id: forecast.id,
              sheet: { kind: "warnings" },
            }),
          )
        : warningsSheet(warnings, timeZone, now, close),
    );
    const newBody = this.dialog.querySelector(".sheet-body");
    if (newBody) newBody.scrollTop = scrollTop;
    if (!this.dialog.open) {
      // Opening the sheet and moving focus into it must not move the page
      // behind it (some mobile browsers scroll to the focused element).
      const pageScroll = window.scrollY;
      this.sheetPageScroll = pageScroll;
      this.sheetScreen = screenOf(route);
      this.dialog.showModal();
      const title = /** @type {HTMLElement | null} */ (this.dialog.querySelector(".sheet-title"));
      title?.setAttribute("tabindex", "-1");
      title?.focus({ preventScroll: true });
      const keep = () => {
        if (window.scrollY !== pageScroll) window.scrollTo(0, pageScroll);
      };
      keep();
      requestAnimationFrame(keep);
    }
  }
}

/**
 * The screen a route shows, without its sheet: the key for remembered
 * scroll positions.
 *
 * @param {Route} route
 */
function screenOf(route) {
  return formatRoute(route.view === "locations" ? route : { ...route, sheet: null });
}

/** Steps from the current history entry back to the list (0 when unknown). */
function historyDepth() {
  const depth = /** @type {{depth?: unknown} | null} */ (history.state)?.depth;
  return typeof depth === "number" && depth > 0 ? depth : 0;
}
