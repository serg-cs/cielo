import {
  requiredElement,
  setDynamicIcon,
} from "./dom.js";

const pageSettleDelayMilliseconds = 120;
const pageSwipeDirectionRatio = 1.25;
const pageSwipeMinimumDistancePixels = 48;
const pageSwipeMinimumFlickDistancePixels = 20;
const pageSwipeMinimumVelocityPixelsPerMillisecond = 0.35;
const hourlyModes = [
  "weather",
  "wind",
  "precipitation",
  "humidity",
  "feels-like",
];
const hourlyModeDetails = {
  weather: { label: "Tiempo", icon: "cloud-sun", explanation: null },
  wind: {
    label: "Viento",
    icon: "wind",
    explanation: "Viento-Rachas (km/h)",
  },
  precipitation: {
    label: "Precipitación",
    icon: "umbrella",
    explanation: "Probabilidad (%) / acumulado (mm)",
  },
  humidity: {
    label: "Humedad",
    icon: "droplet",
    explanation: "Humedad relativa (%)",
  },
  "feels-like": {
    label: "Sensación",
    icon: "thermometer",
    explanation: "Sensación térmica (°C)",
  },
};
const windFlowRotationByDirection = {
  N: 180,
  NE: 225,
  E: 270,
  SE: 315,
  S: 0,
  SO: 45,
  O: 90,
  NO: 135,
};
const precipitationAmountFormatter = new Intl.NumberFormat("es-ES", {
  maximumFractionDigits: 1,
});
const weekdayFormatter = new Intl.DateTimeFormat("es-ES", {
  weekday: "short",
  timeZone: "UTC",
});
const shortDateFormatter = new Intl.DateTimeFormat("es-ES", {
  day: "numeric",
  month: "short",
  timeZone: "UTC",
});
const fullDateFormatter = new Intl.DateTimeFormat("es-ES", {
  weekday: "long",
  day: "numeric",
  month: "long",
  timeZone: "UTC",
});

export class ForecastController {
  #elements;
  #onCloseRequest;
  #onClose;
  #municipality = null;
  #currentConditions = null;
  #dailySummary = null;
  #dailyForecastPeriods = [];
  #hourlyForecastPeriods = [];
  #forecastStatus = "loading";
  #dailyItems = [];
  #hourlyItems = [];
  #hourlyMode = "weather";
  #hourlyModePickerIsOpen = false;
  #pageIndicators = [];
  #pageIndex = 0;
  #settledPageIndex = 0;
  #pageSettleTimeout = null;
  #pageResizeFrame = null;
  #resetHourlyScrollOnRender = false;
  #hourlyScrollResetFrame = null;
  #dailyScrollResetFrame = null;
  #pageSwipe = {
    touchId: null,
    startX: 0,
    startY: 0,
    startTime: 0,
    startPageIndex: 0,
  };

  constructor(root, { onCloseRequest, onClose }) {
    this.#elements = captureForecastElements(root);
    this.#onCloseRequest = onCloseRequest;
    this.#onClose = onClose;
    this.#createPageIndicators();
    this.#installInteractions();
    this.#settlePage(0);
  }

  show(
    municipality,
    currentConditions,
    dailySummary,
    hourlyForecastPeriods,
    dailyForecastPeriods,
    forecastStatus,
  ) {
    this.#municipality = municipality;
    this.#currentConditions = currentConditions;
    this.#dailySummary = dailySummary;
    this.#hourlyForecastPeriods = hourlyForecastPeriods;
    this.#dailyForecastPeriods = dailyForecastPeriods;
    this.#forecastStatus = forecastStatus;
    this.#hourlyMode = "weather";
    this.#closeHourlyModePicker();
    this.#renderHourlyModePicker();
    this.#resetHourlyScrollOnRender = true;
    this.#elements.titleText.textContent = municipality.name;
    this.#elements.title.setAttribute(
      "aria-label",
      `${municipality.name}, ${municipality.province}`,
    );
    this.#renderCurrentConditions();
    this.#renderHourlyForecast();
    this.#renderDailyForecast();
    this.#elements.root.hidden = false;
    this.#elements.root.dataset.active = "true";
    this.#resetPagePosition();
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

  setDailySummary(municipalityId, dailySummary) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#dailySummary = dailySummary;
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

  setDailyForecastPeriods(municipalityId, periods) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#dailyForecastPeriods = periods;
    this.#renderDailyForecast();
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
    this.#dailySummary = null;
    this.#dailyForecastPeriods = [];
    this.#hourlyForecastPeriods = [];
    this.#forecastStatus = "loading";
    this.#hourlyMode = "weather";
    this.#closeHourlyModePicker();
    this.#clearPageTimers();
    document.removeEventListener("keydown", this.#handleDocumentKeydown);
    this.#onClose(municipalityId);
  }

  #requestClose() {
    if (this.#municipality === null) {
      return;
    }

    this.#onCloseRequest();
  }

  #createPageIndicators() {
    this.#pageIndicators = this.#elements.pages.map(() => {
      const indicator = document.createElement("span");
      indicator.className = "forecast-page-dot";
      indicator.setAttribute("aria-hidden", "true");
      this.#elements.pageIndicator.append(indicator);
      return indicator;
    });
    this.#renderPageIndicator(0);
  }

  #installInteractions() {
    this.#elements.locationsButton.addEventListener("click", () => {
      this.#requestClose();
    });
    this.#elements.pageIndicator.addEventListener("click", () => {
      const pageCount = this.#elements.pages.length;
      if (this.#municipality === null || pageCount === 0) {
        return;
      }

      const nextPageIndex = (this.#pageIndex + 1) % pageCount;
      this.#scrollToPage(nextPageIndex, "smooth");
    });
    this.#elements.hourlyModeButton.addEventListener("click", () => {
      if (this.#hourlyModePickerIsOpen) {
        this.#closeHourlyModePicker();
      } else {
        this.#openHourlyModePicker();
      }
    });
    this.#elements.hourlyModeButton.addEventListener(
      "keydown",
      this.#handleHourlyModeButtonKeydown,
    );
    this.#elements.hourlyModeListbox.addEventListener(
      "click",
      this.#handleHourlyModeListboxClick,
    );
    this.#elements.hourlyModeListbox.addEventListener(
      "keydown",
      this.#handleHourlyModeListboxKeydown,
    );
    document.addEventListener("pointerdown", this.#handleDocumentPointerdown);
    this.#elements.pageTrack.addEventListener(
      "scroll",
      this.#handlePageScroll,
      { passive: true },
    );
    this.#elements.pageTrack.addEventListener(
      "touchstart",
      this.#handlePageSwipeStart,
      { passive: true },
    );
    this.#elements.pageTrack.addEventListener(
      "touchend",
      this.#handlePageSwipeEnd,
      { passive: false },
    );
    this.#elements.pageTrack.addEventListener(
      "touchcancel",
      this.#handlePageSwipeCancel,
      { passive: true },
    );
    window.addEventListener("resize", this.#handleWindowResize);
  }

  #installDocumentKeys() {
    document.removeEventListener("keydown", this.#handleDocumentKeydown);
    document.addEventListener("keydown", this.#handleDocumentKeydown);
  }

  #handleDocumentKeydown = (event) => {
    if (this.#municipality === null) {
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      this.#requestClose();
      return;
    }
    if (
      (event.key === "ArrowLeft" || event.key === "ArrowRight") &&
      !pageNavigationIsBlocked(event)
    ) {
      const direction = event.key === "ArrowLeft" ? -1 : 1;
      const nextIndex = clampPageIndex(
        this.#pageIndex + direction,
        this.#elements.pages.length,
      );
      event.preventDefault();
      if (nextIndex !== this.#pageIndex) {
        this.#scrollToPage(nextIndex, "smooth");
      }
    }
  };

  #handleDocumentPointerdown = (event) => {
    if (
      this.#hourlyModePickerIsOpen &&
      event.target instanceof Node &&
      !this.#elements.hourlyModePicker.contains(event.target)
    ) {
      this.#closeHourlyModePicker();
    }
  };

  #handleHourlyModeButtonKeydown = (event) => {
    if (event.key === "Tab" && this.#hourlyModePickerIsOpen) {
      this.#closeHourlyModePicker();
      return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      this.#openHourlyModePicker();
      const selectedIndex = hourlyModes.indexOf(this.#hourlyMode);
      const targetIndex = event.key === "ArrowDown"
        ? selectedIndex
        : Math.max(selectedIndex, 0);
      this.#elements.hourlyModeOptions[targetIndex]?.focus();
      return;
    }
    if (event.key === "Escape" && this.#hourlyModePickerIsOpen) {
      event.preventDefault();
      event.stopPropagation();
      this.#closeHourlyModePicker();
    }
  };

  #handleHourlyModeListboxClick = (event) => {
    if (!(event.target instanceof Element)) {
      return;
    }
    const option = event.target.closest("[data-hourly-mode]");
    if (!(option instanceof HTMLButtonElement)) {
      return;
    }
    this.#selectHourlyMode(option.dataset.hourlyMode);
  };

  #handleHourlyModeListboxKeydown = (event) => {
    const activeIndex = this.#elements.hourlyModeOptions.indexOf(
      document.activeElement,
    );
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      this.#closeHourlyModePicker({ restoreFocus: true });
      return;
    }
    if (event.key === "Tab") {
      this.#closeHourlyModePicker();
      return;
    }
    if (event.key === "Enter" || event.key === " ") {
      const option = this.#elements.hourlyModeOptions[activeIndex];
      if (option !== undefined) {
        event.preventDefault();
        this.#selectHourlyMode(option.dataset.hourlyMode);
      }
      return;
    }

    let nextIndex = activeIndex;
    if (event.key === "ArrowDown") {
      nextIndex = (activeIndex + 1) % hourlyModes.length;
    } else if (event.key === "ArrowUp") {
      nextIndex = (activeIndex - 1 + hourlyModes.length) % hourlyModes.length;
    } else if (event.key === "Home") {
      nextIndex = 0;
    } else if (event.key === "End") {
      nextIndex = hourlyModes.length - 1;
    } else {
      return;
    }
    event.preventDefault();
    this.#elements.hourlyModeOptions[nextIndex]?.focus();
  };

  #handlePageScroll = () => {
    const pageWidth = this.#elements.pageTrack.clientWidth;
    if (pageWidth <= 0) {
      return;
    }

    const pageIndex = clampPageIndex(
      Math.round(this.#elements.pageTrack.scrollLeft / pageWidth),
      this.#elements.pages.length,
    );
    this.#renderPageIndicator(pageIndex);
    if (this.#pageSettleTimeout !== null) {
      window.clearTimeout(this.#pageSettleTimeout);
    }
    this.#pageSettleTimeout = window.setTimeout(() => {
      this.#pageSettleTimeout = null;
      this.#settlePage(pageIndex);
    }, pageSettleDelayMilliseconds);
  };

  #handlePageSwipeStart = (event) => {
    if (
      this.#municipality === null ||
      event.touches.length !== 1 ||
      !(event.target instanceof Element) ||
      event.target.closest(
        "button, a[href], input, select, textarea, [contenteditable], .hourly-scroll",
      ) !== null
    ) {
      this.#resetPageSwipe();
      return;
    }

    const touch = event.touches[0];
    if (touch === undefined) {
      return;
    }

    const pageWidth = this.#elements.pageTrack.clientWidth;
    this.#pageSwipe.touchId = touch.identifier;
    this.#pageSwipe.startX = touch.clientX;
    this.#pageSwipe.startY = touch.clientY;
    this.#pageSwipe.startTime = event.timeStamp;
    this.#pageSwipe.startPageIndex = pageWidth <= 0
      ? this.#pageIndex
      : clampPageIndex(
        Math.round(this.#elements.pageTrack.scrollLeft / pageWidth),
        this.#elements.pages.length,
      );
  };

  #handlePageSwipeEnd = (event) => {
    const touch = this.#pageSwipeTouch(event.changedTouches);
    if (touch === null) {
      return;
    }

    const horizontalDistance = touch.clientX - this.#pageSwipe.startX;
    const horizontalMagnitude = Math.abs(horizontalDistance);
    const verticalMagnitude = Math.abs(touch.clientY - this.#pageSwipe.startY);
    const duration = Math.max(1, event.timeStamp - this.#pageSwipe.startTime);
    const velocity = horizontalMagnitude / duration;
    const startPageIndex = this.#pageSwipe.startPageIndex;
    this.#resetPageSwipe();

    const directionIsHorizontal =
      horizontalMagnitude >= verticalMagnitude * pageSwipeDirectionRatio;
    const distanceIsEnough =
      horizontalMagnitude >= pageSwipeMinimumDistancePixels;
    const flickIsEnough =
      horizontalMagnitude >= pageSwipeMinimumFlickDistancePixels &&
      velocity >= pageSwipeMinimumVelocityPixelsPerMillisecond;
    if (
      !directionIsHorizontal ||
      (!distanceIsEnough && !flickIsEnough)
    ) {
      return;
    }

    const direction = horizontalDistance < 0 ? 1 : -1;
    const targetPageIndex = clampPageIndex(
      startPageIndex + direction,
      this.#elements.pages.length,
    );
    if (targetPageIndex === startPageIndex) {
      return;
    }

    if (event.cancelable) {
      event.preventDefault();
    }
    this.#scrollToPage(targetPageIndex, "smooth");
  };

  #handlePageSwipeCancel = (event) => {
    if (this.#pageSwipeTouch(event.changedTouches) !== null) {
      this.#resetPageSwipe();
    }
  };

  #pageSwipeTouch(touches) {
    if (this.#pageSwipe.touchId === null) {
      return null;
    }

    return Array.from(touches).find(
      (touch) => touch.identifier === this.#pageSwipe.touchId,
    ) ?? null;
  }

  #resetPageSwipe() {
    this.#pageSwipe.touchId = null;
  }

  #handleWindowResize = () => {
    if (this.#municipality === null) {
      return;
    }
    if (this.#pageResizeFrame !== null) {
      window.cancelAnimationFrame(this.#pageResizeFrame);
    }

    this.#pageResizeFrame = window.requestAnimationFrame(() => {
      this.#pageResizeFrame = null;
      this.#scrollToPage(this.#pageIndex, "auto");
      if (this.#hourlyModePickerIsOpen) {
        this.#positionHourlyModeListbox();
      }
    });
  };

  #scrollToPage(pageIndex, behavior) {
    const boundedIndex = clampPageIndex(
      pageIndex,
      this.#elements.pages.length,
    );
    this.#renderPageIndicator(boundedIndex);
    this.#elements.pageTrack.scrollTo({
      left: this.#elements.pageTrack.clientWidth * boundedIndex,
      behavior: reducedMotionIsPreferred() ? "auto" : behavior,
    });
    if (behavior === "auto") {
      this.#settlePage(boundedIndex);
    }
  }

  #renderPageIndicator(pageIndex) {
    this.#pageIndex = pageIndex;
    for (const [index, indicator] of this.#pageIndicators.entries()) {
      indicator.dataset.active = String(index === pageIndex);
    }
  }

  #settlePage(pageIndex) {
    const pageChanged = pageIndex !== this.#settledPageIndex;
    this.#settledPageIndex = pageIndex;
    if (pageIndex !== 0) {
      this.#closeHourlyModePicker();
    }

    // Keep keyboard focus outside a page before removing it from interaction.
    const activeElement = document.activeElement;
    const focusIsLeavingPage =
      activeElement instanceof Element &&
      this.#elements.pages.some(
        (page, index) =>
          index !== pageIndex && page.contains(activeElement),
      );
    if (focusIsLeavingPage) {
      this.#elements.pageTrack.focus({ preventScroll: true });
    }

    for (const [index, page] of this.#elements.pages.entries()) {
      const isActive = index === pageIndex;
      page.inert = !isActive;
      if (isActive) {
        page.removeAttribute("aria-hidden");
      } else {
        page.setAttribute("aria-hidden", "true");
      }
    }
    this.#elements.hourlyScroller.tabIndex = pageIndex === 0 ? 0 : -1;
    this.#elements.dailyScroller.tabIndex = pageIndex === 1 ? 0 : -1;
    if (pageChanged) {
      const page = this.#elements.pages[pageIndex];
      const label = page?.dataset.pageLabel ?? "";
      this.#elements.pageAnnouncement.textContent =
        `Página ${pageIndex + 1} de ${this.#elements.pages.length}. ${label}`;
    }
  }

  #resetPagePosition() {
    this.#clearPageTimers();
    this.#elements.pageAnnouncement.textContent = "";
    this.#pageIndex = 0;
    this.#settledPageIndex = 0;
    this.#elements.pageTrack.scrollLeft = 0;
    this.#resetDailyScroll();
    this.#renderPageIndicator(0);
    this.#settlePage(0);

    // Repeat after layout so reopened forecasts always begin on the first page.
    this.#pageResizeFrame = window.requestAnimationFrame(() => {
      this.#pageResizeFrame = null;
      if (this.#municipality !== null) {
        this.#elements.pageTrack.scrollLeft = 0;
      }
    });
  }

  #clearPageTimers() {
    if (this.#pageSettleTimeout !== null) {
      window.clearTimeout(this.#pageSettleTimeout);
      this.#pageSettleTimeout = null;
    }
    if (this.#pageResizeFrame !== null) {
      window.cancelAnimationFrame(this.#pageResizeFrame);
      this.#pageResizeFrame = null;
    }
    if (this.#dailyScrollResetFrame !== null) {
      window.cancelAnimationFrame(this.#dailyScrollResetFrame);
      this.#dailyScrollResetFrame = null;
    }
    if (this.#hourlyScrollResetFrame !== null) {
      window.cancelAnimationFrame(this.#hourlyScrollResetFrame);
      this.#hourlyScrollResetFrame = null;
    }
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
      precipitationProbability: requiredElement(
        element.querySelector(".hourly-precipitation-probability"),
        HTMLElement,
      ),
      windDirectionArrow: requiredElement(
        element.querySelector(".hourly-wind-direction-arrow"),
        SVGElement,
      ),
      windSpeed: requiredElement(
        element.querySelector(".hourly-wind-speed"),
        HTMLElement,
      ),
      windGust: requiredElement(
        element.querySelector(".hourly-wind-gust"),
        HTMLElement,
      ),
      modePrecipitationProbability: requiredElement(
        element.querySelector(".hourly-mode-precipitation-probability"),
        HTMLElement,
      ),
      precipitationAmount: requiredElement(
        element.querySelector(".hourly-precipitation-amount"),
        HTMLElement,
      ),
      humidity: requiredElement(
        element.querySelector(".hourly-humidity-value"),
        HTMLElement,
      ),
      humidityUnit: requiredElement(
        element.querySelector(".hourly-humidity-unit"),
        HTMLElement,
      ),
      apparentTemperature: requiredElement(
        element.querySelector(".hourly-apparent-temperature"),
        HTMLElement,
      ),
    };
  }

  #openHourlyModePicker() {
    if (this.#municipality === null) {
      return;
    }
    this.#hourlyModePickerIsOpen = true;
    this.#elements.hourlyModeListbox.hidden = false;
    this.#elements.hourlyModeButton.setAttribute("aria-expanded", "true");
    this.#positionHourlyModeListbox();
  }

  #positionHourlyModeListbox() {
    const viewport = this.#elements.currentForecastPage.getBoundingClientRect();
    const button = this.#elements.hourlyModeButton.getBoundingClientRect();
    const rootFontSize = Number.parseFloat(
      window.getComputedStyle(document.documentElement).fontSize,
    );
    const gap = (Number.isFinite(rootFontSize) ? rootFontSize : 16) * 0.4;
    const availableAbove = Math.max(0, button.top - viewport.top - gap);
    const availableBelow = Math.max(0, viewport.bottom - button.bottom - gap);
    const contentHeight = this.#elements.hourlyModeListbox.scrollHeight;
    const placement = contentHeight <= availableAbove ||
        availableAbove >= availableBelow
      ? "above"
      : "below";
    const availableHeight = placement === "above"
      ? availableAbove
      : availableBelow;

    this.#elements.hourlyModeListbox.dataset.placement = placement;
    this.#elements.hourlyModeListbox.style.setProperty(
      "--hourly-mode-listbox-max-height",
      `${Math.floor(availableHeight)}px`,
    );
    this.#revealSelectedHourlyMode();
  }

  #revealSelectedHourlyMode() {
    const selectedOption = this.#elements.hourlyModeOptions.find(
      (option) => option.dataset.hourlyMode === this.#hourlyMode,
    );
    if (selectedOption === undefined) {
      return;
    }
    const listbox = this.#elements.hourlyModeListbox;
    const optionTop = selectedOption.offsetTop;
    const optionBottom = optionTop + selectedOption.offsetHeight;
    if (optionTop < listbox.scrollTop) {
      listbox.scrollTop = optionTop;
    } else if (optionBottom > listbox.scrollTop + listbox.clientHeight) {
      listbox.scrollTop = optionBottom - listbox.clientHeight;
    }
  }

  #closeHourlyModePicker({ restoreFocus = false } = {}) {
    this.#hourlyModePickerIsOpen = false;
    this.#elements.hourlyModeListbox.hidden = true;
    delete this.#elements.hourlyModeListbox.dataset.placement;
    this.#elements.hourlyModeListbox.style.removeProperty(
      "--hourly-mode-listbox-max-height",
    );
    this.#elements.hourlyModeButton.setAttribute("aria-expanded", "false");
    if (restoreFocus && this.#municipality !== null) {
      this.#elements.hourlyModeButton.focus({ preventScroll: true });
    }
  }

  #selectHourlyMode(mode) {
    if (!hourlyModes.includes(mode)) {
      return;
    }
    const scrollAnchor = this.#firstVisibleHourlyForecastKey();
    const modeChanged = mode !== this.#hourlyMode;
    this.#hourlyMode = mode;
    this.#closeHourlyModePicker({ restoreFocus: true });
    this.#renderHourlyModePicker();
    if (modeChanged) {
      this.#renderHourlyForecast();
      this.#restoreHourlyScroll(scrollAnchor);
    }
  }

  #renderHourlyModePicker() {
    const details = hourlyModeDetails[this.#hourlyMode];
    this.#elements.hourlyForecastSection.dataset.hourlyMode = this.#hourlyMode;
    this.#elements.hourlyModeExplanation.textContent = details.explanation ?? "";
    this.#elements.hourlyModeExplanation.hidden = details.explanation === null;
    this.#elements.hourlyModeButton.setAttribute(
      "aria-label",
      `Datos por hora: ${details.label}`,
    );
    setDynamicIcon(this.#elements.hourlyModeCurrentIcon, details.icon);
    for (const option of this.#elements.hourlyModeOptions) {
      option.setAttribute(
        "aria-selected",
        String(option.dataset.hourlyMode === this.#hourlyMode),
      );
    }
  }

  #createDailyItem() {
    const fragment = this.#elements.dailyPeriodTemplate.content.cloneNode(true);
    const element = fragment.querySelector(".daily-period");
    if (!(element instanceof HTMLTableRowElement)) {
      throw new Error("No se pudo crear la previsión diaria");
    }

    return {
      element,
      weekdayCell: requiredElement(
        element.querySelector(".daily-weekday"),
        HTMLTableCellElement,
      ),
      weekday: requiredElement(
        element.querySelector(".daily-weekday-name"),
        HTMLElement,
      ),
      date: requiredElement(
        element.querySelector(".daily-date"),
        HTMLElement,
      ),
      icon: requiredElement(
        element.querySelector(".daily-condition-icon"),
        SVGElement,
      ),
      minimumDescription: requiredElement(
        element.querySelector(".daily-minimum-description"),
        HTMLElement,
      ),
      maximumDescription: requiredElement(
        element.querySelector(".daily-maximum-description"),
        HTMLElement,
      ),
      minimumTemperature: requiredElement(
        element.querySelector(".daily-minimum-temperature"),
        HTMLElement,
      ),
      maximumTemperature: requiredElement(
        element.querySelector(".daily-maximum-temperature"),
        HTMLElement,
      ),
      precipitationProbabilityIcon: requiredElement(
        element.querySelector(".daily-precipitation-probability-icon"),
        SVGElement,
      ),
      precipitationProbability: requiredElement(
        element.querySelector(".daily-precipitation-probability"),
        HTMLElement,
      ),
      precipitationProbabilityDescription: requiredElement(
        element.querySelector(
          ".daily-precipitation-probability-description",
        ),
        HTMLElement,
      ),
    };
  }

  #renderCurrentConditions() {
    const hasCurrentConditions = this.#currentConditions !== null;
    const hasDailySummary = hasCurrentConditions && this.#dailySummary !== null;
    this.#elements.currentReading.hidden = !hasCurrentConditions;
    this.#elements.currentConditionDescription.hidden = !hasCurrentConditions;
    this.#elements.currentDailyExtrema.hidden = !hasDailySummary;
    this.#elements.currentConditionsMessage.hidden = hasCurrentConditions;
    if (this.#currentConditions === null) {
      setDynamicIcon(this.#elements.currentConditionIcon, null);
      this.#elements.currentConditionDescription.textContent = "";
      this.#elements.currentConditionsMessage.textContent =
        this.#forecastStatusMessage;
      this.#elements.currentTemperatureValue.textContent = "";
      this.#elements.currentMinimumTemperature.textContent = "";
      this.#elements.currentMaximumTemperature.textContent = "";
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
    if (this.#dailySummary === null) {
      this.#elements.currentMinimumTemperature.textContent = "";
      this.#elements.currentMaximumTemperature.textContent = "";
      this.#elements.currentTemperatureAnnouncement.textContent =
        `Temperatura actual: ${this.#currentConditions.temperatureCelsius} grados Celsius. ` +
        this.#currentConditions.description;
      return;
    }

    this.#elements.currentMinimumTemperature.textContent =
      `${this.#dailySummary.minimumTemperatureCelsius}°`;
    this.#elements.currentMaximumTemperature.textContent =
      `${this.#dailySummary.maximumTemperatureCelsius}°`;
    this.#elements.currentTemperatureAnnouncement.textContent =
      `Temperatura actual: ${this.#currentConditions.temperatureCelsius} grados Celsius. ` +
      `${this.#currentConditions.description}. ` +
      `Mínima prevista: ${this.#dailySummary.minimumTemperatureCelsius} grados Celsius. ` +
      `Máxima prevista: ${this.#dailySummary.maximumTemperatureCelsius} grados Celsius`;
  }

  #renderHourlyForecast() {
    const hasHourlyForecast = this.#hasHourlyForecast;
    this.#elements.hourlyForecastSection.hidden = !hasHourlyForecast;
    const visiblePeriods = this.#hourlyMode === "weather"
      ? this.#hourlyForecastPeriods
      : this.#hourlyForecastPeriods.filter((period) => period.kind === "forecast");
    this.#hourlyItems = visiblePeriods.map(
      () => this.#createHourlyItem(),
    );
    this.#elements.hourlyList.replaceChildren(
      ...this.#hourlyItems.map(({ element }) => element),
    );

    let forecastIndex = 0;
    for (
      const [
        index,
        {
          element,
          hour,
          icon,
          temperature,
          precipitationProbability,
          windDirectionArrow,
          windSpeed,
          windGust,
          modePrecipitationProbability,
          precipitationAmount,
          humidity,
          humidityUnit,
          apparentTemperature,
        },
      ] of this.#hourlyItems.entries()
    ) {
      const period = visiblePeriods[index];
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
        formatHourlyForecastLabel(period, isCurrent, this.#hourlyMode),
      );
      if (isForecast) {
        element.dataset.forecastKey = hourlyForecastKey(period);
        hour.textContent = isCurrent ? "Ahora" : String(period.hour);
        temperature.textContent = period.forecast === null
          ? "—"
          : `${period.forecast.temperatureCelsius}°`;
        const probabilityText =
          formatVisiblePrecipitationProbability(period.forecast);
        precipitationProbability.textContent = probabilityText;
        element.dataset.hasPrecipitation = String(probabilityText !== "");
        const direction = period.forecast?.windDirection ?? null;
        const arrowRotation = direction === null
          ? undefined
          : windFlowRotationByDirection[direction];
        windDirectionArrow.toggleAttribute("hidden", arrowRotation === undefined);
        if (arrowRotation !== undefined) {
          windDirectionArrow.style.setProperty(
            "--hourly-wind-arrow-rotation",
            `${arrowRotation}deg`,
          );
        } else {
          windDirectionArrow.style.removeProperty(
            "--hourly-wind-arrow-rotation",
          );
        }
        const speed = period.forecast?.windSpeedKilometresPerHour ?? null;
        const gust = period.forecast?.maximumGustKilometresPerHour ?? null;
        windSpeed.textContent = formatWindValue(speed);
        windGust.textContent = formatWindValue(gust);
        modePrecipitationProbability.textContent =
          formatModePrecipitationProbability(period.forecast);
        precipitationAmount.textContent = formatPrecipitationAmount(
          period.forecast?.precipitationMillimetres,
        );
        humidity.textContent = period.forecast?.relativeHumidityPercent === null ||
            period.forecast?.relativeHumidityPercent === undefined
          ? "—"
          : String(period.forecast.relativeHumidityPercent);
        humidityUnit.textContent = period.forecast?.relativeHumidityPercent === null ||
            period.forecast?.relativeHumidityPercent === undefined
          ? ""
          : "%";
        apparentTemperature.textContent =
          period.forecast?.apparentTemperatureCelsius === null ||
            period.forecast?.apparentTemperatureCelsius === undefined
            ? "—"
            : `${period.forecast.apparentTemperatureCelsius}°`;
        setDynamicIcon(icon, period.forecast?.condition ?? null);
        continue;
      }

      hour.textContent = displaySolarTime(period.time);
      temperature.textContent = solarEventLabel(period.kind);
      precipitationProbability.textContent = "";
      element.dataset.hasPrecipitation = "false";
      setDynamicIcon(icon, period.kind);
    }

    if (hasHourlyForecast && this.#resetHourlyScrollOnRender) {
      this.#resetHourlyScroll();
    }
  }

  #renderDailyForecast() {
    this.#dailyItems = this.#dailyForecastPeriods.map(
      () => this.#createDailyItem(),
    );
    this.#elements.dailyList.replaceChildren(
      ...this.#dailyItems.map(({ element }) => element),
    );

    for (const [index, item] of this.#dailyItems.entries()) {
      const period = this.#dailyForecastPeriods[index];
      if (period === undefined) {
        continue;
      }

      item.element.dataset.today = String(period.isToday);
      item.weekday.textContent = period.isToday
        ? "Hoy"
        : formatShortWeekday(period.date);
      item.date.textContent = formatShortDate(period.date);
      const dateLabel = period.isToday
        ? `Hoy, ${formatFullDate(period.date)}`
        : formatFullDate(period.date);
      item.weekdayCell.setAttribute(
        "aria-label",
        period.description === null
          ? dateLabel
          : `${dateLabel}. ${period.description}`,
      );
      setDynamicIcon(item.icon, period.condition);
      setDailyTemperature(
        item.minimumTemperature,
        period.minimumTemperatureCelsius,
      );
      setDailyTemperature(
        item.maximumTemperature,
        period.maximumTemperatureCelsius,
      );
      item.minimumDescription.textContent = dailyTemperatureLabel(
        "Mínima",
        period.minimumTemperatureCelsius,
      );
      item.maximumDescription.textContent = dailyTemperatureLabel(
        "Máxima",
        period.maximumTemperatureCelsius,
      );
      const precipitationProbabilityText =
        period.precipitationProbabilityPercent !== null &&
          period.precipitationProbabilityPercent > 0
          ? `${period.precipitationProbabilityPercent}%`
          : "";
      item.precipitationProbabilityIcon.toggleAttribute(
        "hidden",
        precipitationProbabilityText === "",
      );
      item.precipitationProbability.textContent =
        precipitationProbabilityText;
      item.precipitationProbabilityDescription.textContent =
        dailyPrecipitationProbabilityLabel(period);
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

  #firstVisibleHourlyForecastKey() {
    const scrollLeft = this.#elements.hourlyScroller.scrollLeft;
    const item = this.#hourlyItems.find(({ element }) =>
      element.dataset.forecastKey !== undefined &&
      element.offsetLeft + element.offsetWidth > scrollLeft + 1
    );
    if (item !== undefined) {
      return item.element.dataset.forecastKey ?? null;
    }
    for (let index = this.#hourlyItems.length - 1; index >= 0; index -= 1) {
      const forecastKey = this.#hourlyItems[index]?.element.dataset.forecastKey;
      if (forecastKey !== undefined) {
        return forecastKey;
      }
    }
    return null;
  }

  #restoreHourlyScroll(forecastKey) {
    if (forecastKey === null) {
      return;
    }
    if (this.#hourlyScrollResetFrame !== null) {
      window.cancelAnimationFrame(this.#hourlyScrollResetFrame);
    }
    this.#hourlyScrollResetFrame = window.requestAnimationFrame(() => {
      this.#hourlyScrollResetFrame = null;
      const item = this.#hourlyItems.find(
        ({ element }) => element.dataset.forecastKey === forecastKey,
      );
      if (item !== undefined && this.#municipality !== null) {
        this.#elements.hourlyScroller.scrollLeft = item.element.offsetLeft;
      }
    });
  }

  #resetDailyScroll() {
    if (this.#dailyScrollResetFrame !== null) {
      window.cancelAnimationFrame(this.#dailyScrollResetFrame);
    }

    // Reset after layout as well so a municipality never opens on a later day.
    this.#elements.dailyScroller.scrollTop = 0;
    this.#dailyScrollResetFrame = window.requestAnimationFrame(() => {
      this.#dailyScrollResetFrame = null;
      if (this.#municipality !== null) {
        this.#elements.dailyScroller.scrollTop = 0;
      }
    });
  }

  get #hasHourlyForecast() {
    return this.#hourlyForecastPeriods.some(
      (period) => period.kind === "forecast" && period.forecast !== null,
    );
  }

  get #hasDailyForecast() {
    return this.#dailyForecastPeriods.some(
      (period) => period.minimumTemperatureCelsius !== null,
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
    if (this.#hasHourlyForecast) {
      return "Temperatura actual no disponible";
    }
    return this.#hasDailyForecast
      ? "Previsión por horas no disponible"
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
    pageTrack: requiredElement(
      root.querySelector("#forecast-pages"),
      HTMLElement,
    ),
    pages: Array.from(root.querySelectorAll("[data-forecast-page]")).map(
      (page) => requiredElement(page, HTMLElement),
    ),
    pageIndicator: requiredElement(
      root.querySelector("#forecast-page-indicator"),
      HTMLButtonElement,
    ),
    pageAnnouncement: requiredElement(
      root.querySelector("#forecast-page-announcement"),
      HTMLElement,
    ),
    currentForecastPage: requiredElement(
      root.querySelector(".current-forecast-page"),
      HTMLElement,
    ),
    locationsButton: requiredElement(
      root.querySelector("#locations-button"),
      HTMLButtonElement,
    ),
    title: requiredElement(
      root.querySelector("#municipality-title"),
      HTMLElement,
    ),
    titleText: requiredElement(
      root.querySelector("#municipality-title-text"),
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
    currentDailyExtrema: requiredElement(
      root.querySelector("#current-daily-extrema"),
      HTMLElement,
    ),
    currentMinimumTemperature: requiredElement(
      root.querySelector("#current-minimum-temperature"),
      HTMLElement,
    ),
    currentMaximumTemperature: requiredElement(
      root.querySelector("#current-maximum-temperature"),
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
    hourlyModePicker: requiredElement(
      root.querySelector(".hourly-mode-picker"),
      HTMLElement,
    ),
    hourlyModeExplanation: requiredElement(
      root.querySelector("#hourly-mode-explanation"),
      HTMLElement,
    ),
    hourlyModeButton: requiredElement(
      root.querySelector("#hourly-mode-button"),
      HTMLButtonElement,
    ),
    hourlyModeCurrentIcon: requiredElement(
      root.querySelector(".hourly-mode-current-icon"),
      SVGElement,
    ),
    hourlyModeListbox: requiredElement(
      root.querySelector("#hourly-mode-listbox"),
      HTMLElement,
    ),
    hourlyModeOptions: Array.from(
      root.querySelectorAll(".hourly-mode-option"),
    ).map((option) => requiredElement(option, HTMLButtonElement)),
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
    dailyScroller: requiredElement(
      root.querySelector(".daily-scroll"),
      HTMLElement,
    ),
    dailyList: requiredElement(
      root.querySelector("#daily-forecast-list"),
      HTMLTableSectionElement,
    ),
    dailyPeriodTemplate: requiredElement(
      document.querySelector("#daily-forecast-period-template"),
      HTMLTemplateElement,
    ),
  };
}

function clampPageIndex(pageIndex, pageCount) {
  return Math.min(Math.max(pageIndex, 0), Math.max(pageCount - 1, 0));
}

function pageNavigationIsBlocked(event) {
  if (
    event.altKey ||
    event.ctrlKey ||
    event.metaKey ||
    event.shiftKey ||
    !(event.target instanceof Element)
  ) {
    return true;
  }

  return event.target.closest(
    "button, a[href], input, select, textarea, [contenteditable], .hourly-scroll",
  ) !== null;
}

function reducedMotionIsPreferred() {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

function forecastDate(date) {
  return new Date(Date.UTC(
    Number(date.slice(0, 4)),
    Number(date.slice(5, 7)) - 1,
    Number(date.slice(8, 10)),
  ));
}

function formatShortWeekday(date) {
  const value = weekdayFormatter
    .format(forecastDate(date))
    .replace(/\.$/u, "");
  return value.charAt(0).toLocaleUpperCase("es-ES") + value.slice(1);
}

function formatShortDate(date) {
  return shortDateFormatter
    .format(forecastDate(date))
    .replace(/\./gu, "")
    .toLocaleLowerCase("es-ES");
}

function formatFullDate(date) {
  return fullDateFormatter.format(forecastDate(date));
}

function setDailyTemperature(element, temperatureCelsius) {
  if (temperatureCelsius === null) {
    element.textContent = "—";
    return;
  }

  element.textContent = `${temperatureCelsius}°`;
}

function dailyTemperatureLabel(label, temperatureCelsius) {
  return temperatureCelsius === null
    ? `${label}: no disponible`
    : `${label}: ${temperatureCelsius} grados Celsius`;
}

function formatVisiblePrecipitationProbability(forecast) {
  const percent = forecast?.precipitationProbabilityPercent;
  return percent !== null && percent !== undefined && percent > 0
    ? `${percent}%`
    : "";
}

function formatHourlyForecastLabel(period, isCurrent, mode) {
  if (period.kind !== "forecast") {
    return `${solarEventLabel(period.kind)} a las ${displaySolarTime(period.time)}`;
  }
  const hourLabel = isCurrent
    ? "Ahora"
    : `${period.hour} ${period.hour === 1 ? "hora" : "horas"}`;
  if (period.forecast === null) {
    return `${hourLabel}. Previsión no disponible`;
  }

  if (mode === "wind") {
    const direction = period.forecast.windDirection === null
      ? "Dirección no disponible"
      : `Dirección ${period.forecast.windDirection}`;
    const speed = period.forecast.windSpeedKilometresPerHour === null
      ? "Velocidad no disponible"
      : `Velocidad ${period.forecast.windSpeedKilometresPerHour} kilómetros por hora`;
    const gust = period.forecast.maximumGustKilometresPerHour === null
      ? "Racha máxima no disponible"
      : `Racha máxima ${period.forecast.maximumGustKilometresPerHour} kilómetros por hora`;
    return `${hourLabel}. Viento. ${direction}. ${speed}. ${gust}`;
  }
  if (mode === "precipitation") {
    const probability = period.forecast.precipitationProbabilityPercent === null
      ? "Probabilidad no disponible"
      : `Probabilidad ${period.forecast.precipitationProbabilityPercent} por ciento`;
    const amount = precipitationAmountLabel(
      period.forecast.precipitationMillimetres,
    );
    return `${hourLabel}. Precipitación. ${probability}. ${amount}`;
  }
  if (mode === "humidity") {
    const humidity = period.forecast.relativeHumidityPercent === null
      ? "Humedad relativa no disponible"
      : `Humedad relativa ${period.forecast.relativeHumidityPercent} por ciento`;
    return `${hourLabel}. ${humidity}`;
  }
  if (mode === "feels-like") {
    const apparentTemperature = period.forecast.apparentTemperatureCelsius === null
      ? "Sensación térmica no disponible"
      : `Sensación térmica ${period.forecast.apparentTemperatureCelsius} grados Celsius`;
    return `${hourLabel}. ${apparentTemperature}`;
  }

  const precipitation =
    hourlyPrecipitationProbabilityLabel(period.forecast);
  return `${hourLabel}. ${period.forecast.temperatureCelsius} grados Celsius. ` +
    `${period.forecast.description}${precipitation}`;
}

function hourlyForecastKey(period) {
  return `${period.date}:${period.hour}`;
}

function formatWindValue(speed) {
  return speed === null || speed === undefined ? "—" : String(speed);
}

function formatModePrecipitationProbability(forecast) {
  const percent = forecast?.precipitationProbabilityPercent;
  return percent === null || percent === undefined ? "—" : `${percent}%`;
}

function formatPrecipitationAmount(amount) {
  if (amount === null || amount === undefined) {
    return "—";
  }
  if (amount === "trace") {
    return "Traza";
  }
  return `${precipitationAmountFormatter.format(amount)} mm`;
}

function precipitationAmountLabel(amount) {
  if (amount === null) {
    return "Cantidad acumulada no disponible";
  }
  if (amount === "trace") {
    return "Cantidad acumulada inapreciable";
  }
  return `Cantidad acumulada ${precipitationAmountFormatter.format(amount)} milímetros`;
}

function hourlyPrecipitationProbabilityLabel(forecast) {
  if (
    forecast.precipitationProbabilityPercent !== null &&
    forecast.precipitationProbabilityPeriod !== null
  ) {
    return `. Probabilidad de precipitación: ${
      forecast.precipitationProbabilityPercent
    } por ciento ${precipitationPeriodLabel(
      forecast.precipitationProbabilityPeriod,
    )}`;
  }

  return "";
}

function dailyPrecipitationProbabilityLabel(period) {
  if (
    period.precipitationProbabilityPercent === null ||
    period.precipitationProbabilityPeriod === null
  ) {
    return "";
  }

  if (period.precipitationProbabilityPeriod === "00-24") {
    return "Probabilidad de precipitación durante todo el día: " +
      `${period.precipitationProbabilityPercent} por ciento`;
  }

  const scope = period.isToday ? "para el resto del día" : "durante el día";
  return `Probabilidad de precipitación ${scope}: ${
    period.precipitationProbabilityPercent
  } por ciento, ${precipitationPeriodLabel(
    period.precipitationProbabilityPeriod,
  )}`;
}

function precipitationPeriodLabel(period) {
  if (period === "00-24") {
    return "durante todo el día";
  }

  const start = Number(period.slice(0, 2));
  const end = Number(period.slice(3, 5));
  const nextDay = end <= start ? " del día siguiente" : "";
  return `entre las ${start} y las ${end} horas${nextDay}`;
}

function displaySolarTime(time) {
  return time.startsWith("0") ? time.slice(1) : time;
}

function solarEventLabel(kind) {
  return kind === "sunrise" ? "Amanecer" : "Atardecer";
}
