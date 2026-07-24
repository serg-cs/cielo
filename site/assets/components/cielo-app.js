import {
  getTrackedMunicipalities,
  validateMunicipalities,
} from "../lib/catalog.js";
import {
  readLastOpenedMunicipalityId,
  readTrackedMunicipalityIds,
  saveLastOpenedMunicipalityId,
  saveTrackedMunicipalityIds,
} from "../lib/storage.js";
import {
  fetchValidatedJson,
  readValidatedJson,
} from "../lib/data-cache.js";
import {
  dataUrl,
} from "../lib/config.js";
import {
  CurrentForecastStore,
} from "../lib/weather.js";
import {
  CieloLocationsView,
} from "./cielo-locations-view.js";
import {
  CieloMunicipalityView,
} from "./cielo-municipality-view.js";

const navigationStateKey = "cielo";
const catalogUrl = new URL("municipalities.json", dataUrl);

/**
 * @typedef {object} MunicipalityOpenDetail
 * @property {string} municipalityId
 * @property {boolean} shouldTrack
 */

/**
 * @typedef {object} MunicipalityIdentityDetail
 * @property {string} municipalityId
 */

/**
 * @typedef {object} MunicipalityReorderDetail
 * @property {string} municipalityId
 * @property {number} targetIndex
 */

/**
 * @typedef {object} CurrentForecastChangeDetail
 * @property {string} municipalityId
 * @property {import("../lib/weather.js").CurrentForecast | null} forecast
 */

/**
 * @typedef {object} HourlyForecastChangeDetail
 * @property {string} municipalityId
 * @property {import("../lib/weather.js").HourlyForecastPeriod[]} forecasts
 */

/**
 * @typedef {object} ForecastStatusChangeDetail
 * @property {string} municipalityId
 * @property {import("../lib/weather.js").ForecastStatus} status
 */

/**
 * @typedef {{view: "locations"} | {view: "municipality", municipalityId: string}} NavigationState
 */

export class CieloApp extends HTMLElement {
  #municipalities = [];
  #municipalitiesById = new Map();
  #trackedIds = new Set();
  #selectedId = null;
  #lastOpenedId = null;
  #initialized = false;
  #catalogLoadInFlight = false;
  #catalogLoadFailed = false;
  #currentForecasts = new CurrentForecastStore(dataUrl);

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.#render();
  }

  connectedCallback() {
    window.addEventListener("popstate", this.#handlePopState);
    window.addEventListener("online", this.#handleOnline);
    window.addEventListener("offline", this.#handleOffline);
    if (this.#initialized) {
      if (this.#municipalities.length > 0) {
        this.#currentForecasts.start(this.#municipalities, this.#trackedIds);
      } else {
        void this.#initialize();
      }
      return;
    }

    this.#initialized = true;
    this.#installEventCoordination();
    void this.#initialize();
  }

  disconnectedCallback() {
    window.removeEventListener("popstate", this.#handlePopState);
    window.removeEventListener("online", this.#handleOnline);
    window.removeEventListener("offline", this.#handleOffline);
    this.#currentForecasts.stop();
  }

  /** @returns {CieloLocationsView | null} */
  get #locationsView() {
    const view = this.shadowRoot?.querySelector("cielo-locations-view");
    return view instanceof CieloLocationsView ? view : null;
  }

  /** @returns {CieloMunicipalityView | null} */
  get #municipalityView() {
    const view = this.shadowRoot?.querySelector("cielo-municipality-view");
    return view instanceof CieloMunicipalityView ? view : null;
  }

  #installEventCoordination() {
    this.shadowRoot?.addEventListener("municipality-open", (event) => {
      if (!(event instanceof CustomEvent)) {
        return;
      }

      this.#openMunicipality(/** @type {MunicipalityOpenDetail} */ (event.detail));
    });
    this.shadowRoot?.addEventListener("municipality-remove", (event) => {
      if (!(event instanceof CustomEvent)) {
        return;
      }

      this.#removeMunicipality(
        /** @type {MunicipalityIdentityDetail} */ (event.detail).municipalityId,
      );
    });
    this.shadowRoot?.addEventListener("municipality-reorder", (event) => {
      if (!(event instanceof CustomEvent)) {
        return;
      }

      this.#reorderMunicipality(
        /** @type {MunicipalityReorderDetail} */ (event.detail),
      );
    });
    this.shadowRoot?.addEventListener("municipality-close-request", () => {
      this.#requestMunicipalityClose();
    });
    this.shadowRoot?.addEventListener("municipality-close", (event) => {
      if (!(event instanceof CustomEvent)) {
        return;
      }

      this.#finishMunicipalityClose(
        /** @type {MunicipalityIdentityDetail} */ (event.detail).municipalityId,
      );
    });
    this.shadowRoot?.addEventListener("catalog-retry", () => {
      void this.#initialize();
    });
    this.#currentForecasts.addEventListener(
      "currentforecastchange",
      this.#handleCurrentForecastChange,
    );
    this.#currentForecasts.addEventListener(
      "hourlyforecastchange",
      this.#handleHourlyForecastChange,
    );
    this.#currentForecasts.addEventListener(
      "forecaststatuschange",
      this.#handleForecastStatusChange,
    );
  }

  async #initialize() {
    const locationsView = this.#locationsView;
    if (
      locationsView === null ||
      this.#catalogLoadInFlight ||
      this.#municipalities.length > 0
    ) {
      return;
    }

    this.#catalogLoadInFlight = true;
    locationsView.showLoading();
    try {
      // Render a validated on-device catalog before refreshing it for the next load.
      const cachedMunicipalities = await readValidatedJson(
        catalogUrl,
        validateMunicipalities,
      );
      if (cachedMunicipalities !== null) {
        this.#showCatalog(cachedMunicipalities);
        void this.#refreshCachedCatalog();
        return;
      }

      const municipalities = await fetchValidatedJson(
        catalogUrl,
        validateMunicipalities,
      );
      this.#showCatalog(municipalities);
    } catch (error) {
      console.error(error);
      this.#catalogLoadFailed = true;
      locationsView.showError(
        navigator.onLine
          ? "No se pudieron cargar los municipios"
          : "Sin conexión a Internet",
      );
      this.dataset.ready = "true";
      this.#replaceNavigationState({ view: "locations" });
      this.#setThemeColor("--cielo-color-locations-background");
    } finally {
      this.#catalogLoadInFlight = false;
    }
  }

  /** @param {import("../lib/catalog.js").Municipality[]} municipalities */
  #showCatalog(municipalities) {
    const locationsView = this.#locationsView;
    if (locationsView === null) {
      return;
    }

    this.#municipalities = municipalities;
    this.#catalogLoadFailed = false;
    this.#municipalitiesById = new Map(
      this.#municipalities.map((municipality) => [municipality.id, municipality]),
    );
    this.#trackedIds = new Set(
      [...readTrackedMunicipalityIds()].filter((id) =>
        this.#municipalitiesById.has(id)
      ),
    );
    const storedLastOpenedId = readLastOpenedMunicipalityId();
    this.#lastOpenedId = storedLastOpenedId !== null &&
        this.#trackedIds.has(storedLastOpenedId)
      ? storedLastOpenedId
      : null;
    if (storedLastOpenedId !== null && this.#lastOpenedId === null) {
      saveLastOpenedMunicipalityId(null);
    }

    // Render stable saved state before applying initial navigation.
    locationsView.catalog = this.#municipalities;
    locationsView.trackedIds = this.#trackedIds;
    locationsView.showReady();
    if (this.isConnected) {
      this.#currentForecasts.start(this.#municipalities, this.#trackedIds);
    }
    this.dataset.ready = "true";
    this.#initializeNavigation();
  }

  async #refreshCachedCatalog() {
    if (!navigator.onLine) {
      return;
    }

    try {
      // The next navigation receives the refreshed, validated catalog.
      await fetchValidatedJson(catalogUrl, validateMunicipalities);
    } catch (error) {
      console.warn("No se pudo actualizar el catálogo guardado", error);
    }
  }

  #initializeNavigation() {
    const navigationState = readNavigationState(window.history.state);
    if (
      navigationState?.view === "municipality" &&
      this.#trackedIds.has(navigationState.municipalityId)
    ) {
      this.#rememberMunicipality(navigationState.municipalityId);
      this.#showMunicipality(navigationState.municipalityId);
      return;
    }

    // Establish an in-app locations entry before opening the startup location.
    this.#replaceNavigationState({ view: "locations" });
    const initialMunicipalityId = this.#lastOpenedId ?? getTrackedMunicipalities(
      this.#trackedIds,
      this.#municipalitiesById,
    )[0]?.id;
    if (initialMunicipalityId === undefined) {
      this.#setThemeColor("--cielo-color-locations-background");
      return;
    }

    this.#pushMunicipalityState(initialMunicipalityId);
    this.#rememberMunicipality(initialMunicipalityId);
    this.#showMunicipality(initialMunicipalityId);
  }

  /** @param {MunicipalityOpenDetail} detail */
  #openMunicipality({ municipalityId, shouldTrack }) {
    if (!this.#municipalitiesById.has(municipalityId)) {
      return;
    }

    if (shouldTrack) {
      this.#trackedIds.add(municipalityId);
      saveTrackedMunicipalityIds(this.#trackedIds);
      this.#currentForecasts.setTrackedIds(this.#trackedIds);
    }

    const locationsView = this.#locationsView;
    if (locationsView !== null) {
      locationsView.clearSearch();
      locationsView.trackedIds = this.#trackedIds;
    }
    this.#pushMunicipalityState(municipalityId);
    this.#rememberMunicipality(municipalityId);
    this.#showMunicipality(municipalityId);
  }

  /** @param {string} municipalityId */
  #showMunicipality(municipalityId) {
    const municipality = this.#municipalitiesById.get(municipalityId);
    const locationsView = this.#locationsView;
    const municipalityView = this.#municipalityView;
    if (
      municipality === undefined ||
      locationsView === null ||
      municipalityView === null
    ) {
      return;
    }

    locationsView.closeSwipeRows();
    this.#selectedId = municipalityId;
    this.#setThemeColor("--cielo-color-municipality-background");
    municipalityView.show(
      municipality,
      this.#currentForecasts.getCurrentForecast(municipalityId),
      this.#currentForecasts.getHourlyForecast(municipalityId),
      this.#currentForecasts.getForecastStatus(municipalityId),
    );
    locationsView.inert = true;
    locationsView.setAttribute("aria-hidden", "true");
  }

  #requestMunicipalityClose() {
    if (this.#selectedId === null) {
      return;
    }

    const navigationState = readNavigationState(window.history.state);
    if (navigationState?.view === "municipality") {
      window.history.back();
      return;
    }

    this.#showLocations();
  }

  #handlePopState = (event) => {
    const navigationState = readNavigationState(event.state);
    if (
      navigationState?.view === "municipality" &&
      this.#trackedIds.has(navigationState.municipalityId)
    ) {
      this.#rememberMunicipality(navigationState.municipalityId);
      this.#showMunicipality(navigationState.municipalityId);
      return;
    }

    // Normalize stale detail entries whose municipality is no longer saved.
    if (navigationState?.view === "municipality") {
      this.#replaceNavigationState({ view: "locations" });
    }
    this.#showLocations();
  };

  #handleCurrentForecastChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }

    const { municipalityId, forecast } = /** @type {CurrentForecastChangeDetail} */ (
      event.detail
    );
    this.#locationsView?.setCurrentForecast(municipalityId, forecast);
    this.#municipalityView?.setCurrentForecast(municipalityId, forecast);
  };

  #handleHourlyForecastChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }

    const { municipalityId, forecasts } = /** @type {HourlyForecastChangeDetail} */ (
      event.detail
    );
    this.#municipalityView?.setHourlyForecast(municipalityId, forecasts);
  };

  #handleForecastStatusChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }

    const { municipalityId, status } = /** @type {ForecastStatusChangeDetail} */ (
      event.detail
    );
    this.#municipalityView?.setForecastStatus(municipalityId, status);
  };

  #handleOnline = () => {
    if (this.#catalogLoadFailed) {
      void this.#initialize();
    }
  };

  #handleOffline = () => {
    if (this.#catalogLoadFailed) {
      this.#locationsView?.showError("Sin conexión a Internet");
    }
  };

  #showLocations() {
    if (this.#selectedId === null) {
      this.#locationsView?.removeAttribute("inert");
      this.#locationsView?.removeAttribute("aria-hidden");
      this.#setThemeColor("--cielo-color-locations-background");
      return;
    }

    this.#municipalityView?.dismiss();
  }

  /** @param {string} municipalityId */
  #removeMunicipality(municipalityId) {
    const removedIndex = getTrackedMunicipalities(
      this.#trackedIds,
      this.#municipalitiesById,
    ).findIndex((municipality) => municipality.id === municipalityId);
    if (!this.#trackedIds.delete(municipalityId)) {
      return;
    }

    saveTrackedMunicipalityIds(this.#trackedIds);
    this.#currentForecasts.setTrackedIds(this.#trackedIds);
    if (this.#lastOpenedId === municipalityId) {
      this.#lastOpenedId = null;
      saveLastOpenedMunicipalityId(null);
    }
    if (this.#locationsView !== null) {
      this.#locationsView.trackedIds = this.#trackedIds;
      this.#locationsView.focusAfterRemoval(removedIndex);
    }
  }

  /** @param {MunicipalityReorderDetail} detail */
  #reorderMunicipality({ municipalityId, targetIndex }) {
    const orderedIds = [...this.#trackedIds];
    const sourceIndex = orderedIds.indexOf(municipalityId);
    if (
      sourceIndex === -1 ||
      !Number.isInteger(targetIndex) ||
      targetIndex < 0 ||
      targetIndex >= orderedIds.length ||
      sourceIndex === targetIndex
    ) {
      return;
    }

    // Rebuild the set so its iteration order remains the persisted list order.
    orderedIds.splice(sourceIndex, 1);
    orderedIds.splice(targetIndex, 0, municipalityId);
    this.#trackedIds = new Set(orderedIds);
    saveTrackedMunicipalityIds(this.#trackedIds);
    if (this.#locationsView !== null) {
      this.#locationsView.trackedIds = this.#trackedIds;
      this.#locationsView.restoreFocus(municipalityId);
    }
  }

  /** @param {string} municipalityId */
  #finishMunicipalityClose(municipalityId) {
    if (this.#selectedId !== municipalityId) {
      return;
    }

    this.#selectedId = null;
    if (this.#locationsView !== null) {
      this.#locationsView.inert = false;
      this.#locationsView.removeAttribute("aria-hidden");
      this.#locationsView.restoreFocus(municipalityId);
    }
    this.#setThemeColor("--cielo-color-locations-background");
  }

  /** @param {string} municipalityId */
  #rememberMunicipality(municipalityId) {
    this.#lastOpenedId = municipalityId;
    saveLastOpenedMunicipalityId(municipalityId);
  }

  /** @param {string} municipalityId */
  #pushMunicipalityState(municipalityId) {
    window.history.pushState(
      withNavigationState({ view: "municipality", municipalityId }),
      "",
      window.location.href,
    );
  }

  /** @param {NavigationState} navigationState */
  #replaceNavigationState(navigationState) {
    window.history.replaceState(
      withNavigationState(navigationState),
      "",
      window.location.href,
    );
  }

  /** @param {string} customProperty */
  #setThemeColor(customProperty) {
    const themeColor = document.querySelector('meta[name="theme-color"]');
    const color = getComputedStyle(document.documentElement)
      .getPropertyValue(customProperty)
      .trim();
    if (themeColor !== null && color.length > 0) {
      themeColor.setAttribute("content", color);
    }
  }

  #render() {
    if (this.shadowRoot === null) {
      return;
    }

    this.shadowRoot.innerHTML = `
      <style>
        :host {
          position: relative;
          display: block;
          width: 100%;
          height: 100%;
          overflow: hidden;
          isolation: isolate;
          background: var(--cielo-color-locations-background);
        }
      </style>
      <cielo-locations-view></cielo-locations-view>
      <cielo-municipality-view></cielo-municipality-view>
    `;
  }
}

/** @param {unknown} value @returns {NavigationState | null} */
function readNavigationState(value) {
  if (
    typeof value !== "object" ||
    value === null ||
    !(navigationStateKey in value)
  ) {
    return null;
  }

  const navigationState = value[navigationStateKey];
  if (
    typeof navigationState !== "object" ||
    navigationState === null ||
    !("view" in navigationState)
  ) {
    return null;
  }

  if (navigationState.view === "locations") {
    return { view: "locations" };
  }
  if (
    navigationState.view === "municipality" &&
    "municipalityId" in navigationState &&
    typeof navigationState.municipalityId === "string"
  ) {
    return {
      view: "municipality",
      municipalityId: navigationState.municipalityId,
    };
  }

  return null;
}

/** @param {NavigationState} navigationState */
function withNavigationState(navigationState) {
  const currentState = window.history.state;
  const state = typeof currentState === "object" && currentState !== null
    ? currentState
    : {};
  return { ...state, [navigationStateKey]: navigationState };
}

customElements.define("cielo-app", CieloApp);
