import {
  requiredElement,
  setDynamicIcon,
} from "./dom.js";

const backSwipeEdgeWidth = 32;
const backSwipeMinimumDistance = 64;
const backSwipeDirectionRatio = 1.25;

export class ForecastController {
  #elements;
  #onCloseRequest;
  #onClose;
  #municipality = null;
  #currentConditions = null;
  #hourlyForecastPeriods = [];
  #forecastStatus = "loading";
  #hourlyItems = [];
  #resetHourlyScrollOnRender = false;
  #hourlyScrollResetFrame = null;
  #backSwipe = {
    touchId: null,
    startX: 0,
    startY: 0,
  };

  constructor(root, { onCloseRequest, onClose }) {
    this.#elements = captureForecastElements(root);
    this.#onCloseRequest = onCloseRequest;
    this.#onClose = onClose;
    this.#installInteractions();
  }

  show(
    municipality,
    currentConditions,
    hourlyForecastPeriods,
    forecastStatus,
  ) {
    this.#municipality = municipality;
    this.#currentConditions = currentConditions;
    this.#hourlyForecastPeriods = hourlyForecastPeriods;
    this.#forecastStatus = forecastStatus;
    this.#resetHourlyScrollOnRender = true;
    this.#elements.titleText.textContent = municipality.name;
    this.#elements.title.setAttribute(
      "aria-label",
      `Cambiar ubicación. Ubicación actual: ${municipality.name}, ${municipality.province}`,
    );
    this.#renderCurrentConditions();
    this.#renderHourlyForecast();
    this.#elements.root.hidden = false;
    this.#elements.root.dataset.active = "true";
    this.#installDocumentKeys();

    this.#elements.screen.focus({ preventScroll: true });
  }

  dismiss() {
    if (
      this.#municipality === null ||
      this.#elements.root.hidden
    ) {
      return;
    }

    this.#finishDismiss(this.#municipality.id);
  }

  setCurrentConditions(municipalityId, currentConditions) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#currentConditions = currentConditions;
    this.#renderCurrentConditions();
  }

  setHourlyForecastPeriods(municipalityId, periods) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#hourlyForecastPeriods = periods;
    this.#renderHourlyForecast();
    this.#renderCurrentConditions();
  }

  setForecastStatus(municipalityId, forecastStatus) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#forecastStatus = forecastStatus;
    this.#renderCurrentConditions();
  }

  #finishDismiss(municipalityId) {
    this.#elements.root.hidden = true;
    delete this.#elements.root.dataset.active;
    this.#municipality = null;
    this.#currentConditions = null;
    this.#hourlyForecastPeriods = [];
    this.#forecastStatus = "loading";
    document.removeEventListener("keydown", this.#handleDocumentKeydown);
    this.#onClose(municipalityId);
  }

  #requestClose() {
    if (this.#municipality === null) {
      return;
    }

    this.#onCloseRequest();
  }

  #installInteractions() {
    this.#elements.title.addEventListener("click", () => {
      this.#requestClose();
    });
    this.#elements.locationsButton.addEventListener("click", () => {
      this.#requestClose();
    });

    // Capture edge touches before Safari starts native history navigation.
    this.#elements.screen.addEventListener(
      "touchstart",
      this.#handleBackSwipeStart,
      { passive: false },
    );
    this.#elements.screen.addEventListener(
      "touchmove",
      this.#handleBackSwipeMove,
      { passive: false },
    );
    this.#elements.screen.addEventListener(
      "touchend",
      this.#handleBackSwipeEnd,
      { passive: false },
    );
    this.#elements.screen.addEventListener(
      "touchcancel",
      this.#handleBackSwipeCancel,
      { passive: false },
    );
  }

  #installDocumentKeys() {
    document.removeEventListener("keydown", this.#handleDocumentKeydown);
    document.addEventListener("keydown", this.#handleDocumentKeydown);
  }

  #handleDocumentKeydown = (event) => {
    if (this.#municipality !== null && event.key === "Escape") {
      event.preventDefault();
      this.#requestClose();
    }
  };

  #handleBackSwipeStart = (event) => {
    if (
      this.#municipality === null ||
      event.touches.length !== 1 ||
      !(event.target instanceof Element) ||
      event.target.closest(
        "button, a[href], input, select, textarea, [contenteditable]",
      ) !== null
    ) {
      return;
    }

    const touch = event.touches[0];
    if (touch === undefined || touch.clientX > backSwipeEdgeWidth) {
      return;
    }

    this.#backSwipe.touchId = touch.identifier;
    this.#backSwipe.startX = touch.clientX;
    this.#backSwipe.startY = touch.clientY;
    event.preventDefault();
  };

  #handleBackSwipeMove = (event) => {
    if (this.#backSwipe.touchId === null) {
      return;
    }

    event.preventDefault();
  };

  #handleBackSwipeEnd = (event) => {
    const touch = this.#backSwipeTouch(event.changedTouches);
    if (touch === null) {
      return;
    }

    const horizontalDistance = touch.clientX - this.#backSwipe.startX;
    const verticalDistance = Math.abs(touch.clientY - this.#backSwipe.startY);
    this.#resetBackSwipe();
    event.preventDefault();
    if (
      horizontalDistance >= backSwipeMinimumDistance &&
      horizontalDistance >= verticalDistance * backSwipeDirectionRatio
    ) {
      this.#requestClose();
    }
  };

  #handleBackSwipeCancel = (event) => {
    if (this.#backSwipeTouch(event.changedTouches) === null) {
      return;
    }

    this.#resetBackSwipe();
  };

  #backSwipeTouch(touches) {
    if (this.#backSwipe.touchId === null) {
      return null;
    }

    return Array.from(touches).find(
      (touch) => touch.identifier === this.#backSwipe.touchId,
    ) ?? null;
  }

  #resetBackSwipe() {
    this.#backSwipe.touchId = null;
  }

  #createHourlyItem() {
    const fragment = this.#elements.hourlyPeriodTemplate.content.cloneNode(true);
    const element = fragment.firstElementChild;
    if (!(element instanceof HTMLLIElement)) {
      throw new Error("No se pudo crear la previsión por horas");
    }

    return {
      element,
      hour: requiredElement(
        element.querySelector(".hourly-hour"),
        HTMLElement,
      ),
      icon: requiredElement(
        element.querySelector(".hourly-condition-icon"),
        SVGElement,
      ),
      temperature: requiredElement(
        element.querySelector(".hourly-temperature"),
        HTMLElement,
      ),
    };
  }

  #renderCurrentConditions() {
    const hasCurrentConditions = this.#currentConditions !== null;
    this.#elements.currentReading.hidden = !hasCurrentConditions;
    this.#elements.currentConditionDescription.hidden = !hasCurrentConditions;
    this.#elements.currentConditionsMessage.hidden = hasCurrentConditions;
    if (this.#currentConditions === null) {
      setDynamicIcon(this.#elements.currentConditionIcon, null);
      this.#elements.currentConditionDescription.textContent = "";
      this.#elements.currentConditionsMessage.textContent =
        this.#forecastStatusMessage;
      this.#elements.currentTemperatureValue.textContent = "";
      this.#elements.currentTemperatureAnnouncement.textContent = "";
      return;
    }

    setDynamicIcon(
      this.#elements.currentConditionIcon,
      this.#currentConditions.condition,
    );
    this.#elements.currentConditionDescription.textContent =
      this.#currentConditions.description;
    this.#elements.currentConditionsMessage.textContent = "";
    this.#elements.currentTemperatureValue.textContent =
      `${this.#currentConditions.temperatureCelsius}°`;
    this.#elements.currentTemperatureAnnouncement.textContent =
      `Temperatura actual: ${this.#currentConditions.temperatureCelsius} grados Celsius`;
  }

  #renderHourlyForecast() {
    const hasHourlyForecast = this.#hasHourlyForecast;
    this.#elements.hourlyForecastSection.hidden = !hasHourlyForecast;
    this.#hourlyItems = this.#hourlyForecastPeriods.map(
      () => this.#createHourlyItem(),
    );
    this.#elements.hourlyList.replaceChildren(
      ...this.#hourlyItems.map(({ element }) => element),
    );

    let forecastIndex = 0;
    for (const [index, { element, hour, icon, temperature }] of
      this.#hourlyItems.entries()) {
      const period = this.#hourlyForecastPeriods[index];
      if (period === undefined) {
        continue;
      }
      const isForecast = period.kind === "forecast";
      const isCurrent = isForecast && forecastIndex === 0;
      if (isForecast) {
        forecastIndex += 1;
      }

      element.dataset.kind = period.kind;
      element.dataset.current = String(isCurrent);
      element.setAttribute(
        "aria-label",
        formatHourlyForecastLabel(period, isCurrent),
      );
      if (isForecast) {
        hour.textContent = isCurrent ? "Ahora" : String(period.hour);
        temperature.textContent = period.forecast === null
          ? "—"
          : `${period.forecast.temperatureCelsius}°`;
        setDynamicIcon(icon, period.forecast?.condition ?? null);
        continue;
      }

      hour.textContent = displaySolarTime(period.time);
      temperature.textContent = solarEventLabel(period.kind);
      setDynamicIcon(icon, period.kind);
    }

    if (hasHourlyForecast && this.#resetHourlyScrollOnRender) {
      this.#resetHourlyScroll();
    }
  }

  #resetHourlyScroll() {
    this.#resetHourlyScrollOnRender = false;
    if (this.#hourlyScrollResetFrame !== null) {
      window.cancelAnimationFrame(this.#hourlyScrollResetFrame);
    }

    // Reset immediately and once after layout so reopened forecasts always start at Ahora.
    this.#elements.hourlyScroller.scrollLeft = 0;
    this.#hourlyScrollResetFrame = window.requestAnimationFrame(() => {
      this.#hourlyScrollResetFrame = null;
      if (this.#municipality !== null) {
        this.#elements.hourlyScroller.scrollLeft = 0;
      }
    });
  }

  get #hasHourlyForecast() {
    return this.#hourlyForecastPeriods.some(
      (period) => period.kind === "forecast" && period.forecast !== null,
    );
  }

  get #forecastStatusMessage() {
    if (this.#forecastStatus === "loading") {
      return "Cargando previsión…";
    }
    if (this.#forecastStatus === "offline") {
      return "Sin conexión a Internet";
    }
    if (this.#forecastStatus === "error") {
      return "No se pudo cargar la previsión";
    }
    return this.#hasHourlyForecast
      ? "Temperatura actual no disponible"
      : "Previsión no disponible";
  }
}

function captureForecastElements(root) {
  return {
    root,
    screen: requiredElement(
      root.querySelector(".forecast-screen"),
      HTMLElement,
    ),
    title: requiredElement(
      root.querySelector("#municipality-switcher"),
      HTMLButtonElement,
    ),
    locationsButton: requiredElement(
      root.querySelector("#locations-button"),
      HTMLButtonElement,
    ),
    titleText: requiredElement(
      root.querySelector("#municipality-title"),
      HTMLElement,
    ),
    currentReading: requiredElement(
      root.querySelector(".current-reading"),
      HTMLElement,
    ),
    currentConditionIcon: requiredElement(
      root.querySelector("#current-condition-icon"),
      SVGElement,
    ),
    currentConditionDescription: requiredElement(
      root.querySelector("#current-condition-description"),
      HTMLElement,
    ),
    currentConditionsMessage: requiredElement(
      root.querySelector("#current-conditions-message"),
      HTMLElement,
    ),
    currentTemperatureValue: requiredElement(
      root.querySelector("#current-temperature-value"),
      HTMLElement,
    ),
    currentTemperatureAnnouncement: requiredElement(
      root.querySelector("#current-temperature-announcement"),
      HTMLElement,
    ),
    hourlyForecastSection: requiredElement(
      root.querySelector("#hourly-forecast"),
      HTMLElement,
    ),
    hourlyScroller: requiredElement(
      root.querySelector(".hourly-scroll"),
      HTMLElement,
    ),
    hourlyList: requiredElement(
      root.querySelector("#hourly-forecast-list"),
      HTMLUListElement,
    ),
    hourlyPeriodTemplate: requiredElement(
      document.querySelector("#hourly-forecast-period-template"),
      HTMLTemplateElement,
    ),
  };
}

function formatHourlyForecastLabel(period, isCurrent) {
  if (period.kind !== "forecast") {
    return `${solarEventLabel(period.kind)} a las ${displaySolarTime(period.time)}`;
  }
  const hourLabel = isCurrent
    ? "Ahora"
    : `${period.hour} ${period.hour === 1 ? "hora" : "horas"}`;
  if (period.forecast === null) {
    return `${hourLabel}. Previsión no disponible`;
  }

  return `${hourLabel}. ${period.forecast.temperatureCelsius} grados Celsius. ${period.forecast.description}`;
}

function displaySolarTime(time) {
  return time.startsWith("0") ? time.slice(1) : time;
}

function solarEventLabel(kind) {
  return kind === "sunrise" ? "Amanecer" : "Atardecer";
}
