import {
  requiredElement,
  setDynamicIcon,
} from "./dom.js";

const backSwipeEdgeWidth = 32;
const backSwipeMinimumDistance = 64;
const backSwipeDirectionRatio = 1.25;
const pageSettleDelayMilliseconds = 120;
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
  #pageIndicators = [];
  #pageIndex = 0;
  #settledPageIndex = 0;
  #pageSettleTimeout = null;
  #pageResizeFrame = null;
  #resetHourlyScrollOnRender = false;
  #hourlyScrollResetFrame = null;
  #dailyScrollResetFrame = null;
  #backSwipe = {
    touchId: null,
    startX: 0,
    startY: 0,
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
    this.#resetHourlyScrollOnRender = true;
    this.#elements.titleText.textContent = municipality.name;
    this.#elements.title.setAttribute(
      "aria-label",
      `Cambiar ubicación. Ubicación actual: ${municipality.name}, ${municipality.province}`,
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
      this.#elements.pageIndicator.append(indicator);
      return indicator;
    });
    this.#renderPageIndicator(0);
  }

  #installInteractions() {
    this.#elements.title.addEventListener("click", () => {
      this.#requestClose();
    });
    this.#elements.locationsButton.addEventListener("click", () => {
      this.#requestClose();
    });
    this.#elements.pageTrack.addEventListener(
      "scroll",
      this.#handlePageScroll,
      { passive: true },
    );
    window.addEventListener("resize", this.#handleWindowResize);

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
      if (nextIndex !== this.#pageIndex) {
        event.preventDefault();
        this.#scrollToPage(nextIndex, "smooth");
      }
    }
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
  }

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
      if (this.#pageIndex === 0) {
        this.#requestClose();
        return;
      }
      this.#scrollToPage(this.#pageIndex - 1, "smooth");
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
      HTMLElement,
    ),
    pageAnnouncement: requiredElement(
      root.querySelector("#forecast-page-announcement"),
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
