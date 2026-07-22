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
  CurrentTemperatureStore,
} from "../lib/weather.js";
import {
  CieloLocationsView,
} from "./cielo-locations-view.js";
import {
  CieloMunicipalityView,
} from "./cielo-municipality-view.js";

const navigationStateKey = "cielo";

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
 * @typedef {object} TemperatureChangeDetail
 * @property {string} municipalityId
 * @property {number | null} celsius
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
  #temperatures = new CurrentTemperatureStore();

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.#render();
  }

  connectedCallback() {
    window.addEventListener("popstate", this.#handlePopState);
    if (this.#initialized) {
      if (this.#municipalities.length > 0) {
        this.#temperatures.start(this.#municipalities, this.#trackedIds);
      }
      return;
    }

    this.#initialized = true;
    this.#installEventCoordination();
    void this.#initialize();
  }

  disconnectedCallback() {
    window.removeEventListener("popstate", this.#handlePopState);
    this.#temperatures.stop();
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
    this.#temperatures.addEventListener(
      "temperaturechange",
      this.#handleTemperatureChange,
    );
  }

  async #initialize() {
    const locationsView = this.#locationsView;
    if (locationsView === null) {
      return;
    }

    try {
      // Validate and index the catalog before exposing any interactive control.
      const response = await fetch("./data/municipalities.json", {
        cache: "no-cache",
      });
      if (!response.ok) {
        throw new Error(
          `No se pudieron cargar los municipios: HTTP ${response.status}`,
        );
      }

      this.#municipalities = validateMunicipalities(await response.json());
      this.#municipalitiesById = new Map(
        this.#municipalities.map((municipality) => [municipality.id, municipality]),
      );
      this.#trackedIds = new Set(
        [...readTrackedMunicipalityIds()].filter((id) =>
          this.#municipalitiesById.has(id),
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
        this.#temperatures.start(this.#municipalities, this.#trackedIds);
      }
      this.dataset.ready = "true";
      this.#initializeNavigation();
    } catch (error) {
      console.error(error);
      locationsView.showError();
      this.dataset.ready = "true";
      this.#replaceNavigationState({ view: "locations" });
      this.#setThemeColor("--cielo-color-locations-background");
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
      this.#temperatures.setTrackedIds(this.#trackedIds);
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
      this.#temperatures.getCurrentTemperature(municipalityId),
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

  #handleTemperatureChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }

    const { municipalityId, celsius } = /** @type {TemperatureChangeDetail} */ (
      event.detail
    );
    this.#locationsView?.setTemperature(municipalityId, celsius);
    this.#municipalityView?.setTemperature(municipalityId, celsius);
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
    this.#temperatures.setTrackedIds(this.#trackedIds);
    if (this.#lastOpenedId === municipalityId) {
      this.#lastOpenedId = null;
      saveLastOpenedMunicipalityId(null);
    }
    if (this.#locationsView !== null) {
      this.#locationsView.trackedIds = this.#trackedIds;
      this.#locationsView.focusAfterRemoval(removedIndex);
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
