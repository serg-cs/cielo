import {
  getSavedMunicipalities,
  validateMunicipalityCatalog,
} from "./municipality-catalog.js";
import {
  readLastOpenedMunicipalityId,
  readSavedMunicipalityIds,
  saveLastOpenedMunicipalityId,
  saveSavedMunicipalityIds,
} from "./preferences-store.js";
import {
  fetchValidatedJson,
  readValidatedJson,
} from "./weather-data-client.js";
import {
  ForecastStore,
} from "./forecast-store.js";
import {
  LocationsController,
} from "./locations-controller.js";
import {
  ForecastController,
} from "./forecast-controller.js";
import {
  requiredElement,
} from "./dom.js";

const navigationStateKey = "cielo";

export class ApplicationController {
  #elements;
  #weatherDataUrl;
  #municipalityCatalogUrl;
  #locationsController;
  #forecastController;
  #forecastStore;
  #municipalities = [];
  #municipalitiesById = new Map();
  #savedMunicipalityIds = new Set();
  #selectedMunicipalityId = null;
  #lastOpenedMunicipalityId = null;
  #catalogLoadInFlight = false;
  #catalogLoadFailed = false;

  constructor(root) {
    this.#elements = captureApplicationElements(root);
    this.#weatherDataUrl = new URL(
      requiredWeatherDataUrl(root),
      window.location.href,
    );
    this.#municipalityCatalogUrl = new URL(
      "catalog.json",
      this.#weatherDataUrl,
    );
    this.#forecastStore = new ForecastStore(this.#weatherDataUrl);
    this.#locationsController = new LocationsController(
      this.#elements.locationsView,
      {
        onOpen: (detail) => {
          this.#openMunicipality(detail);
        },
        onRemove: (municipalityId) => {
          this.#removeMunicipality(municipalityId);
        },
        onReorder: (detail) => {
          this.#reorderMunicipality(detail);
        },
        onRetry: () => {
          void this.#initialize();
        },
      },
    );
    this.#forecastController = new ForecastController(
      this.#elements.forecastView,
      {
        onCloseRequest: () => {
          this.#requestForecastClose();
        },
        onClose: (municipalityId) => {
          this.#finishForecastClose(municipalityId);
        },
      },
    );
    this.#installEventCoordination();
  }

  start() {
    window.addEventListener("popstate", this.#handlePopState);
    window.addEventListener("online", this.#handleOnline);
    window.addEventListener("offline", this.#handleOffline);
    void this.#initialize();
  }

  #installEventCoordination() {
    this.#forecastStore.addEventListener(
      "currentconditionschange",
      this.#handleCurrentConditionsChange,
    );
    this.#forecastStore.addEventListener(
      "dailysummarychange",
      this.#handleDailySummaryChange,
    );
    this.#forecastStore.addEventListener(
      "hourlyforecastchange",
      this.#handleHourlyForecastChange,
    );
    this.#forecastStore.addEventListener(
      "forecaststatuschange",
      this.#handleForecastStatusChange,
    );
  }

  async #initialize() {
    if (
      this.#catalogLoadInFlight ||
      this.#municipalities.length > 0
    ) {
      return;
    }

    this.#catalogLoadInFlight = true;
    this.#locationsController.showLoading();
    try {
      const cachedCatalog = await readValidatedJson(
        this.#municipalityCatalogUrl,
        validateMunicipalityCatalog,
      );
      if (cachedCatalog !== null) {
        this.#showCatalog(cachedCatalog);
        void this.#refreshCachedCatalog();
        return;
      }

      const catalog = await fetchValidatedJson(
        this.#municipalityCatalogUrl,
        validateMunicipalityCatalog,
      );
      this.#showCatalog(catalog);
    } catch (error) {
      console.error(error);
      this.#catalogLoadFailed = true;
      this.#locationsController.showError(
        navigator.onLine
          ? "No se pudieron cargar los municipios"
          : "Sin conexión a Internet",
      );
      this.#elements.root.dataset.ready = "true";
      this.#replaceNavigationState({ view: "locations" });
      this.#setThemeColor("--cielo-color-locations-background");
    } finally {
      this.#catalogLoadInFlight = false;
    }
  }

  #showCatalog(catalog) {
    const { municipalities } = catalog;
    this.#municipalities = municipalities;
    this.#catalogLoadFailed = false;
    this.#municipalitiesById = new Map(
      municipalities.map((municipality) => [municipality.id, municipality]),
    );
    this.#savedMunicipalityIds = new Set(
      [...readSavedMunicipalityIds()].filter((municipalityId) =>
        this.#municipalitiesById.has(municipalityId)
      ),
    );
    const storedLastOpenedMunicipalityId = readLastOpenedMunicipalityId();
    this.#lastOpenedMunicipalityId =
      storedLastOpenedMunicipalityId !== null &&
        this.#savedMunicipalityIds.has(storedLastOpenedMunicipalityId)
        ? storedLastOpenedMunicipalityId
        : null;
    if (
      storedLastOpenedMunicipalityId !== null &&
      this.#lastOpenedMunicipalityId === null
    ) {
      saveLastOpenedMunicipalityId(null);
    }

    this.#locationsController.catalog = municipalities;
    this.#locationsController.savedMunicipalityIds =
      this.#savedMunicipalityIds;
    this.#locationsController.setSourceUpdatedAt(catalog.generatedAt);
    this.#locationsController.showReady();
    this.#forecastStore.start(
      this.#municipalities,
      this.#savedMunicipalityIds,
    );
    this.#elements.root.dataset.ready = "true";
    this.#initializeNavigation();
  }

  async #refreshCachedCatalog() {
    if (!navigator.onLine) {
      return;
    }

    try {
      const catalog = await fetchValidatedJson(
        this.#municipalityCatalogUrl,
        validateMunicipalityCatalog,
      );
      this.#locationsController.setSourceUpdatedAt(catalog.generatedAt);
    } catch (error) {
      console.warn("No se pudo actualizar el catálogo guardado", error);
    }
  }

  #initializeNavigation() {
    const navigationState = readNavigationState(window.history.state);
    if (
      navigationState?.view === "forecast" &&
      this.#savedMunicipalityIds.has(navigationState.municipalityId)
    ) {
      this.#rememberMunicipality(navigationState.municipalityId);
      this.#showForecast(navigationState.municipalityId);
      return;
    }

    this.#replaceNavigationState({ view: "locations" });
    const initialMunicipalityId = this.#lastOpenedMunicipalityId ??
      getSavedMunicipalities(
        this.#savedMunicipalityIds,
        this.#municipalitiesById,
      )[0]?.id;
    if (initialMunicipalityId === undefined) {
      this.#setThemeColor("--cielo-color-locations-background");
      return;
    }

    this.#pushForecastState(initialMunicipalityId);
    this.#rememberMunicipality(initialMunicipalityId);
    this.#showForecast(initialMunicipalityId);
  }

  #openMunicipality({ municipalityId, shouldSave }) {
    if (!this.#municipalitiesById.has(municipalityId)) {
      return;
    }

    if (shouldSave) {
      this.#savedMunicipalityIds.add(municipalityId);
      saveSavedMunicipalityIds(this.#savedMunicipalityIds);
      this.#forecastStore.setSavedMunicipalityIds(
        this.#savedMunicipalityIds,
      );
    }

    this.#locationsController.clearSearch();
    this.#locationsController.savedMunicipalityIds =
      this.#savedMunicipalityIds;
    this.#pushForecastState(municipalityId);
    this.#rememberMunicipality(municipalityId);
    this.#showForecast(municipalityId);
  }

  #showForecast(municipalityId) {
    const municipality = this.#municipalitiesById.get(municipalityId);
    if (municipality === undefined) {
      return;
    }

    this.#locationsController.closeSwipeRows();
    this.#selectedMunicipalityId = municipalityId;
    this.#setThemeColor("--cielo-color-forecast-background");
    this.#forecastController.show(
      municipality,
      this.#forecastStore.getCurrentConditions(municipalityId),
      this.#forecastStore.getCurrentDaySummary(municipalityId),
      this.#forecastStore.getHourlyForecastPeriods(municipalityId),
      this.#forecastStore.getForecastStatus(municipalityId),
    );
    this.#elements.locationsView.inert = true;
    this.#elements.locationsView.setAttribute("aria-hidden", "true");
  }

  #requestForecastClose() {
    if (this.#selectedMunicipalityId === null) {
      return;
    }

    const navigationState = readNavigationState(window.history.state);
    if (navigationState?.view === "forecast") {
      window.history.back();
      return;
    }

    this.#showLocations();
  }

  #showLocations() {
    if (this.#selectedMunicipalityId === null) {
      this.#elements.locationsView.inert = false;
      this.#elements.locationsView.removeAttribute("aria-hidden");
      this.#setThemeColor("--cielo-color-locations-background");
      return;
    }

    this.#forecastController.dismiss();
  }

  #removeMunicipality(municipalityId) {
    const removedIndex = getSavedMunicipalities(
      this.#savedMunicipalityIds,
      this.#municipalitiesById,
    ).findIndex((municipality) => municipality.id === municipalityId);
    if (!this.#savedMunicipalityIds.delete(municipalityId)) {
      return;
    }

    saveSavedMunicipalityIds(this.#savedMunicipalityIds);
    this.#forecastStore.setSavedMunicipalityIds(
      this.#savedMunicipalityIds,
    );
    if (this.#lastOpenedMunicipalityId === municipalityId) {
      this.#lastOpenedMunicipalityId = null;
      saveLastOpenedMunicipalityId(null);
    }
    this.#locationsController.savedMunicipalityIds =
      this.#savedMunicipalityIds;
    this.#locationsController.focusAfterRemoval(removedIndex);
  }

  #reorderMunicipality({ municipalityId, targetIndex }) {
    const orderedIds = [...this.#savedMunicipalityIds];
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

    orderedIds.splice(sourceIndex, 1);
    orderedIds.splice(targetIndex, 0, municipalityId);
    this.#savedMunicipalityIds = new Set(orderedIds);
    saveSavedMunicipalityIds(this.#savedMunicipalityIds);
    this.#locationsController.savedMunicipalityIds =
      this.#savedMunicipalityIds;
    this.#locationsController.restoreFocus(municipalityId);
  }

  #finishForecastClose(municipalityId) {
    if (this.#selectedMunicipalityId !== municipalityId) {
      return;
    }

    this.#selectedMunicipalityId = null;
    this.#elements.locationsView.inert = false;
    this.#elements.locationsView.removeAttribute("aria-hidden");
    this.#locationsController.restoreFocus(municipalityId);
    this.#setThemeColor("--cielo-color-locations-background");
  }

  #rememberMunicipality(municipalityId) {
    this.#lastOpenedMunicipalityId = municipalityId;
    saveLastOpenedMunicipalityId(municipalityId);
  }

  #pushForecastState(municipalityId) {
    window.history.pushState(
      withNavigationState({ view: "forecast", municipalityId }),
      "",
      window.location.href,
    );
  }

  #replaceNavigationState(navigationState) {
    window.history.replaceState(
      withNavigationState(navigationState),
      "",
      window.location.href,
    );
  }

  #setThemeColor(customProperty) {
    const color = getComputedStyle(document.documentElement)
      .getPropertyValue(customProperty)
      .trim();
    if (color.length > 0) {
      this.#elements.themeColor.setAttribute("content", color);
    }
  }

  #handlePopState = (event) => {
    const navigationState = readNavigationState(event.state);
    if (
      navigationState?.view === "forecast" &&
      this.#savedMunicipalityIds.has(navigationState.municipalityId)
    ) {
      this.#rememberMunicipality(navigationState.municipalityId);
      this.#showForecast(navigationState.municipalityId);
      return;
    }

    if (navigationState?.view === "forecast") {
      this.#replaceNavigationState({ view: "locations" });
    }
    this.#showLocations();
  };

  #handleCurrentConditionsChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }
    const { municipalityId, currentConditions } = event.detail;
    this.#locationsController.setCurrentConditions(
      municipalityId,
      currentConditions,
    );
    this.#forecastController.setCurrentConditions(
      municipalityId,
      currentConditions,
    );
  };

  #handleHourlyForecastChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }
    const { municipalityId, periods } = event.detail;
    this.#forecastController.setHourlyForecastPeriods(
      municipalityId,
      periods,
    );
  };

  #handleDailySummaryChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }
    const { municipalityId, dailySummary } = event.detail;
    this.#forecastController.setDailySummary(
      municipalityId,
      dailySummary,
    );
  };

  #handleForecastStatusChange = (event) => {
    if (!(event instanceof CustomEvent)) {
      return;
    }
    const { municipalityId, status } = event.detail;
    this.#forecastController.setForecastStatus(municipalityId, status);
  };

  #handleOnline = () => {
    if (this.#catalogLoadFailed) {
      void this.#initialize();
    }
  };

  #handleOffline = () => {
    if (this.#catalogLoadFailed) {
      this.#locationsController.showError("Sin conexión a Internet");
    }
  };
}

function captureApplicationElements(root) {
  return {
    root,
    locationsView: requiredElement(
      root.querySelector("#locations-view"),
      HTMLElement,
    ),
    forecastView: requiredElement(
      root.querySelector("#forecast-view"),
      HTMLElement,
    ),
    themeColor: requiredElement(
      document.querySelector('meta[name="theme-color"]'),
      HTMLMetaElement,
    ),
  };
}

function requiredWeatherDataUrl(root) {
  const value = root.dataset.weatherDataUrl;
  if (value === undefined || value.length === 0) {
    throw new Error("La URL de datos meteorológicos no está configurada");
  }
  return value;
}

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
    navigationState.view === "forecast" &&
    "municipalityId" in navigationState &&
    typeof navigationState.municipalityId === "string"
  ) {
    return {
      view: "forecast",
      municipalityId: navigationState.municipalityId,
    };
  }

  return null;
}

function withNavigationState(navigationState) {
  const currentState = window.history.state;
  const state = typeof currentState === "object" && currentState !== null
    ? currentState
    : {};
  return { ...state, [navigationStateKey]: navigationState };
}
