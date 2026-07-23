const edgeSwipeWidth = 28;
const gestureSlop = 8;
const screenDismissDistance = 96;
const screenDismissVelocity = 0.5;

/** @typedef {import("../lib/catalog.js").Municipality} Municipality */
/** @typedef {import("../lib/weather.js").CurrentForecast} CurrentForecast */
/** @typedef {import("../lib/weather.js").HourlyForecastPeriod} HourlyForecastPeriod */
/** @typedef {import("../lib/weather.js").ForecastStatus} ForecastStatus */

export class CieloMunicipalityView extends HTMLElement {
  #municipality = null;
  #currentForecast = null;
  #hourlyForecast = [];
  #forecastStatus = "loading";
  #hiding = false;
  #edgeDismiss = false;
  #transitionToken = 0;
  #transitionEndHandler = null;
  #transitionFallbackId = null;
  #gesture = {
    pointerId: null,
    startX: 0,
    startY: 0,
    startTime: 0,
    offset: 0,
    dragging: false,
    rejected: false,
  };

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.#render();
    this.#installInteractions();
  }

  connectedCallback() {
    if (this.#municipality === null) {
      this.hidden = true;
      return;
    }

    this.#installDocumentKeys();
  }

  disconnectedCallback() {
    document.removeEventListener("keydown", this.#handleDocumentKeydown);
    this.#cancelPendingTransition();
  }

  /**
   * @param {Municipality} municipality
   * @param {CurrentForecast | null} currentForecast
   * @param {HourlyForecastPeriod[]} hourlyForecast
   * @param {ForecastStatus} forecastStatus
   */
  show(municipality, currentForecast, hourlyForecast, forecastStatus) {
    const screen = this.#screen;
    const title = this.#title;
    const titleText = this.#titleText;
    if (screen === null || title === null || titleText === null) {
      return;
    }

    // Replace any superseded edge transition before showing the next screen.
    this.#cancelPendingTransition();
    this.#municipality = municipality;
    this.#currentForecast = currentForecast;
    this.#hourlyForecast = hourlyForecast;
    this.#forecastStatus = forecastStatus;
    this.#hiding = false;
    this.#edgeDismiss = false;
    this.#transitionToken += 1;
    titleText.textContent = municipality.name;
    title.setAttribute(
      "aria-label",
      `Cambiar ubicación. Ubicación actual: ${municipality.name}, ${municipality.province}`,
    );
    this.#renderCurrentForecast();
    this.#renderHourlyForecast();
    this.hidden = false;
    screen.dataset.dragging = "false";
    screen.dataset.edgeDismiss = "false";
    this.#installDocumentKeys();

    // Present route changes immediately while resetting interactive offset.
    screen.style.setProperty("--screen-offset-x", "0px");
    if (this.#hourlyScroller !== null) {
      this.#hourlyScroller.scrollLeft = 0;
    }
    this.#focusScreen();
  }

  dismiss() {
    const screen = this.#screen;
    if (
      this.#municipality === null ||
      this.#hiding ||
      this.hidden ||
      screen === null
    ) {
      return;
    }

    // Freeze the identity before the screen leaves and invalidate stale work.
    const municipalityId = this.#municipality.id;
    const transitionToken = this.#transitionToken + 1;
    this.#cancelPendingTransition();
    this.#hiding = true;
    this.#transitionToken = transitionToken;
    screen.dataset.dragging = "false";
    const edgeDismiss = this.#edgeDismiss;
    this.#edgeDismiss = false;
    screen.dataset.edgeDismiss = String(edgeDismiss);
    if (!edgeDismiss) {
      this.#finishDismiss(municipalityId);
      return;
    }

    // Complete a committed edge gesture with an interruption fallback.
    const finish = (event) => {
      if (event instanceof TransitionEvent) {
        if (event.target !== screen || event.propertyName !== "transform") {
          return;
        }
      }

      this.#cancelPendingTransition();
      if (transitionToken === this.#transitionToken) {
        this.#finishDismiss(municipalityId);
      }
    };
    this.#transitionEndHandler = finish;
    screen.addEventListener("transitionend", finish);
    screen.style.setProperty("--screen-offset-x", `${screen.offsetWidth}px`);
    this.#transitionFallbackId = window.setTimeout(finish, 380);
  }

  /** @param {string} municipalityId */
  #finishDismiss(municipalityId) {
    const screen = this.#screen;
    if (screen === null) {
      return;
    }

    this.hidden = true;
    this.#municipality = null;
    this.#currentForecast = null;
    this.#hourlyForecast = [];
    this.#forecastStatus = "loading";
    this.#hiding = false;
    screen.style.setProperty("--screen-offset-x", "0px");
    screen.dataset.edgeDismiss = "false";
    document.removeEventListener("keydown", this.#handleDocumentKeydown);
    this.dispatchEvent(
      new CustomEvent("municipality-close", {
        bubbles: true,
        composed: true,
        detail: { municipalityId },
      }),
    );
  }

  /** @param {string} municipalityId @param {CurrentForecast | null} currentForecast */
  setCurrentForecast(municipalityId, currentForecast) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#currentForecast = currentForecast;
    this.#renderCurrentForecast();
  }

  /**
   * @param {string} municipalityId
   * @param {HourlyForecastPeriod[]} hourlyForecast
   */
  setHourlyForecast(municipalityId, hourlyForecast) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#hourlyForecast = hourlyForecast;
    this.#renderHourlyForecast();
    this.#renderCurrentForecast();
  }

  /** @param {string} municipalityId @param {ForecastStatus} forecastStatus */
  setForecastStatus(municipalityId, forecastStatus) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#forecastStatus = forecastStatus;
    this.#renderCurrentForecast();
  }

  /** @param {{edgeSwipe?: boolean}} [options] */
  #requestClose({ edgeSwipe = false } = {}) {
    if (this.#municipality === null || this.#hiding) {
      return;
    }

    this.#edgeDismiss = edgeSwipe;
    this.dispatchEvent(
      new CustomEvent("municipality-close-request", {
        bubbles: true,
        composed: true,
      }),
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

  #installInteractions() {
    const edge = this.shadowRoot?.querySelector(".back-swipe-region");
    if (!(edge instanceof HTMLElement)) {
      return;
    }

    this.#title?.addEventListener("click", () => {
      this.#requestClose();
    });
    this.#locationsButton?.addEventListener("click", () => {
      this.#requestClose();
    });

    edge.addEventListener("pointerdown", (event) => {
      // Qualify primary-button gestures while the full-screen view is stable.
      if (
        !event.isPrimary ||
        event.button !== 0 ||
        this.#municipality === null ||
        this.#hiding
      ) {
        return;
      }

      // Record distance and timing origins for an interactive edge transition.
      this.#gesture.pointerId = event.pointerId;
      this.#gesture.startX = event.clientX;
      this.#gesture.startY = event.clientY;
      this.#gesture.startTime = event.timeStamp;
      this.#gesture.offset = 0;
      this.#gesture.dragging = false;
      this.#gesture.rejected = false;
    });

    edge.addEventListener("pointermove", (event) => {
      // Continue only the active, unrejected edge-pointer sequence.
      if (
        this.#gesture.pointerId !== event.pointerId ||
        this.#gesture.rejected
      ) {
        return;
      }

      // Wait through slop, then accept only a rightward horizontal drag.
      const horizontalDistance = event.clientX - this.#gesture.startX;
      const verticalDistance = event.clientY - this.#gesture.startY;
      if (!this.#gesture.dragging) {
        if (
          Math.abs(horizontalDistance) < gestureSlop &&
          Math.abs(verticalDistance) < gestureSlop
        ) {
          return;
        }

        if (
          horizontalDistance <= 0 ||
          Math.abs(verticalDistance) >= horizontalDistance
        ) {
          this.#gesture.rejected = true;
          return;
        }

        // Capture after back-navigation intent is established.
        this.#gesture.dragging = true;
        edge.setPointerCapture(event.pointerId);
        this.#setDragging(true);
      }

      // Track the screen directly under the pointer while revealing locations.
      this.#gesture.offset = Math.max(0, horizontalDistance);
      this.#screen?.style.setProperty(
        "--screen-offset-x",
        `${this.#gesture.offset}px`,
      );
      event.preventDefault();
    });

    // Settle completed or cancelled edge swipes into a screen or reset state.
    edge.addEventListener("pointerup", (event) => {
      this.#settleGesture(event);
    });
    edge.addEventListener("pointercancel", (event) => {
      this.#settleGesture(event, true);
    });
  }

  /** @param {PointerEvent} event @param {boolean} [cancelled] */
  #settleGesture(event, cancelled = false) {
    if (this.#gesture.pointerId !== event.pointerId) {
      return;
    }

    // Release capture before resolving the interactive transition.
    const edge = this.shadowRoot?.querySelector(".back-swipe-region");
    if (edge instanceof HTMLElement && edge.hasPointerCapture(event.pointerId)) {
      edge.releasePointerCapture(event.pointerId);
    }

    // Commit by distance or velocity, otherwise restore the current screen.
    let committed = false;
    if (this.#gesture.dragging && !cancelled) {
      const duration = Math.max(1, event.timeStamp - this.#gesture.startTime);
      const velocity = this.#gesture.offset / duration;
      committed = this.#gesture.offset >= screenDismissDistance ||
        velocity >= screenDismissVelocity;
    }
    if (this.#gesture.dragging) {
      this.#setDragging(false);
      if (committed) {
        this.#requestClose({ edgeSwipe: true });
      } else {
        this.#resetPosition();
      }
    }

    // Reset pointer qualification for the next edge gesture.
    this.#gesture.pointerId = null;
    this.#gesture.dragging = false;
    this.#gesture.rejected = false;
  }

  /** @param {boolean} dragging */
  #setDragging(dragging) {
    this.#screen?.setAttribute("data-dragging", String(dragging));
  }

  #resetPosition() {
    this.#screen?.style.setProperty("--screen-offset-x", "0px");
  }

  #focusScreen() {
    this.#screen?.focus({ preventScroll: true });
  }

  #renderCurrentForecast() {
    const conditionIcon = this.shadowRoot?.querySelector(
      "#current-condition-icon",
    );
    const value = this.shadowRoot?.querySelector("#current-temperature-value");
    const description = this.shadowRoot?.querySelector(
      "#current-condition-description",
    );
    const announcement = this.shadowRoot?.querySelector(
      "#current-temperature-announcement",
    );
    const reading = this.shadowRoot?.querySelector(".current-reading");
    const message = this.shadowRoot?.querySelector("#current-forecast-message");
    if (
      !(conditionIcon instanceof HTMLElement) ||
      !(value instanceof HTMLElement) ||
      !(description instanceof HTMLElement) ||
      !(announcement instanceof HTMLElement) ||
      !(reading instanceof HTMLElement) ||
      !(message instanceof HTMLElement)
    ) {
      return;
    }

    const hasCurrentForecast = this.#currentForecast !== null;
    reading.hidden = !hasCurrentForecast;
    conditionIcon.hidden = !hasCurrentForecast;
    description.hidden = !hasCurrentForecast;
    message.hidden = hasCurrentForecast;
    if (this.#currentForecast === null) {
      conditionIcon.removeAttribute("name");
      description.textContent = "";
      message.textContent = this.#forecastStatusMessage;
    } else {
      conditionIcon.setAttribute("name", this.#currentForecast.state);
      description.textContent = this.#currentForecast.description;
      message.textContent = "";
    }
    value.textContent = this.#currentForecast === null
      ? ""
      : `${this.#currentForecast.celsius}°`;
    announcement.textContent = this.#currentForecast === null
      ? ""
      : `Temperatura actual: ${this.#currentForecast.celsius} grados Celsius`;
  }

  #renderHourlyForecast() {
    const section = this.shadowRoot?.querySelector("#hourly-forecast");
    const list = this.shadowRoot?.querySelector("#hourly-forecast-list");
    if (!(section instanceof HTMLElement) || !(list instanceof HTMLUListElement)) {
      return;
    }

    // Keep an empty timeline out of the visual and accessibility trees.
    section.hidden = !this.#hasHourlyForecast;
    const periods = this.#hourlyForecast.map((period, index) => {
      const item = document.createElement("li");
      const hour = document.createElement("span");
      const icon = document.createElement("cielo-icon");
      const temperature = document.createElement("span");
      const isCurrent = index === 0;
      const hourLabel = isCurrent ? "Ahora" : String(period.hour);

      item.className = "hourly-period";
      item.dataset.current = String(isCurrent);
      item.setAttribute("aria-label", formatHourlyForecastLabel(period, isCurrent));
      hour.className = "hourly-hour";
      hour.textContent = hourLabel;
      icon.className = "hourly-condition-icon";
      temperature.className = "hourly-temperature";
      temperature.textContent = period.forecast === null
        ? "—"
        : `${period.forecast.celsius}°`;
      if (period.forecast === null) {
        icon.hidden = true;
      } else {
        icon.setAttribute("name", period.forecast.state);
      }
      hour.setAttribute("aria-hidden", "true");
      icon.setAttribute("aria-hidden", "true");
      temperature.setAttribute("aria-hidden", "true");
      item.append(hour, icon, temperature);
      return item;
    });
    list.replaceChildren(...periods);
  }

  get #hasHourlyForecast() {
    return this.#hourlyForecast.some((period) => period.forecast !== null);
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

  #cancelPendingTransition() {
    const screen = this.#screen;
    if (screen !== null && this.#transitionEndHandler !== null) {
      screen.removeEventListener("transitionend", this.#transitionEndHandler);
      this.#transitionEndHandler = null;
    }
    if (this.#transitionFallbackId !== null) {
      window.clearTimeout(this.#transitionFallbackId);
      this.#transitionFallbackId = null;
    }
  }

  /** @returns {HTMLElement | null} */
  get #screen() {
    const screen = this.shadowRoot?.querySelector(".screen");
    return screen instanceof HTMLElement ? screen : null;
  }

  /** @returns {HTMLButtonElement | null} */
  get #title() {
    const title = this.shadowRoot?.querySelector("#municipality-switcher");
    return title instanceof HTMLButtonElement ? title : null;
  }

  /** @returns {HTMLButtonElement | null} */
  get #locationsButton() {
    const button = this.shadowRoot?.querySelector("#locations-button");
    return button instanceof HTMLButtonElement ? button : null;
  }

  /** @returns {HTMLElement | null} */
  get #titleText() {
    const title = this.shadowRoot?.querySelector("#municipality-title");
    return title instanceof HTMLElement ? title : null;
  }

  /** @returns {HTMLElement | null} */
  get #hourlyScroller() {
    const scroller = this.shadowRoot?.querySelector(".hourly-scroll");
    return scroller instanceof HTMLElement ? scroller : null;
  }

  #render() {
    if (this.shadowRoot === null) {
      return;
    }

    this.shadowRoot.innerHTML = `
      <style>
        :host {
          position: absolute;
          z-index: 10;
          inset: 0;
          display: block;
          color-scheme: dark;
        }

        :host([hidden]) {
          display: none;
        }

        * {
          box-sizing: border-box;
        }

        .visually-hidden {
          position: absolute;
          width: 1px;
          height: 1px;
          padding: 0;
          margin: -1px;
          overflow: hidden;
          clip: rect(0, 0, 0, 0);
          white-space: nowrap;
          border: 0;
        }

        .screen {
          position: fixed;
          inset: 0;
          overflow: hidden;
          outline: none;
          color: var(--cielo-color-text);
          background: var(--cielo-color-municipality-background);
          transform: translateX(var(--screen-offset-x, 0));
          transition: transform 220ms cubic-bezier(0.2, 0.82, 0.2, 1);
          will-change: transform;
        }

        .screen[data-edge-dismiss="true"] {
          transition: transform 320ms cubic-bezier(0.2, 0.82, 0.2, 1);
        }

        .screen[data-dragging="true"] {
          transition: none;
        }

        .back-swipe-region {
          position: absolute;
          z-index: 1;
          inset: 0 auto 0 0;
          width: ${edgeSwipeWidth}px;
          touch-action: pan-y;
        }

        .header {
          position: relative;
          z-index: 2;
          width: 100%;
          max-width: var(--cielo-content-width);
          min-height: 5rem;
          padding: calc(0.8rem + env(safe-area-inset-top)) var(--cielo-space-4) 1rem;
          margin-inline: auto;
        }

        .header-content {
          display: flex;
          align-items: center;
          gap: 0.35rem;
          min-height: var(--cielo-touch-target);
        }

        h1 {
          min-width: 0;
          margin: 0;
        }

        .locations-button,
        .location-switcher {
          border: 0;
          outline: none;
          outline-offset: 0.12rem;
          color: inherit;
          background: transparent;
          cursor: pointer;
          -webkit-tap-highlight-color: transparent;
        }

        .locations-button {
          display: grid;
          width: var(--cielo-touch-target);
          height: var(--cielo-touch-target);
          padding: 0;
          border-radius: 50%;
          place-items: center;
        }

        .locations-button cielo-icon {
          width: 1.2rem;
          height: 1.2rem;
        }

        .location-switcher {
          display: flex;
          align-items: center;
          gap: 0.3rem;
          max-width: 100%;
          min-height: var(--cielo-touch-target);
          padding: 0.2rem 0.4rem;
          border-radius: 0.75rem;
          font-size: clamp(1.25rem, 5vw, 1.55rem);
          font-weight: 680;
          letter-spacing: -0.025em;
          line-height: 1.1;
          text-align: left;
        }

        .locations-button:focus-visible,
        .location-switcher:focus-visible {
          outline: var(--cielo-focus-outline);
        }

        .locations-button:active,
        .location-switcher:active {
          opacity: 0.78;
        }

        .title-text {
          min-width: 0;
          overflow: hidden;
          text-overflow: ellipsis;
          white-space: nowrap;
          overflow-wrap: anywhere;
        }

        .current-forecast {
          width: 100%;
          margin-top: 0.55rem;
        }

        .current-reading {
          display: flex;
          align-items: center;
        }

        .current-reading[hidden] {
          display: none;
        }

        .current-condition-icon {
          width: clamp(4.5rem, 18vw, 6.5rem);
          height: clamp(4.5rem, 18vw, 6.5rem);
          margin-left: auto;
          margin-right: 0.9rem;
        }

        .current-condition-icon[hidden] {
          display: none;
        }

        #current-temperature-value {
          flex: 0 0 auto;
          margin-left: 0.9rem;
          font-size: clamp(6rem, 30vw, 10rem);
          font-variant-numeric: tabular-nums;
          font-weight: 300;
          letter-spacing: -0.075em;
          line-height: 0.9;
        }

        .current-condition-description {
          display: block;
          /* Follow a two-digit reading until the desktop text cap takes over. */
          max-width: min(18rem, 41.5vw, calc(100% - 1.8rem));
          margin: 0.65rem 0 0 0.9rem;
          color: var(--cielo-color-muted);
          font-size: clamp(0.95rem, 3.5vw, 1.125rem);
          font-weight: 350;
          letter-spacing: 0.005em;
          line-height: 1.35;
          overflow-wrap: anywhere;
        }

        .current-condition-description[hidden] {
          display: none;
        }

        .current-forecast-message {
          display: block;
          padding: 2.2rem 0.9rem;
          color: var(--cielo-color-muted);
          font-size: var(--cielo-font-size-small);
          line-height: 1.4;
          text-align: center;
        }

        .current-forecast-message[hidden] {
          display: none;
        }

        .hourly-forecast {
          position: fixed;
          bottom: calc(1.9rem + env(safe-area-inset-bottom));
          left: 50%;
          width: calc(100% - 3.8rem);
          max-width: calc(var(--cielo-content-width) - 3.8rem);
          margin: 0;
          transform: translateX(-50%);
        }

        .hourly-forecast[hidden] {
          display: none;
        }

        .hourly-scroll {
          width: 100%;
          overflow-x: auto;
          border: 1px solid var(--cielo-color-border);
          border-radius: 1rem;
          outline: none;
          outline-offset: -0.2rem;
          overscroll-behavior-x: contain;
          scrollbar-width: none;
          scroll-snap-type: x proximity;
          -webkit-overflow-scrolling: touch;
        }

        .hourly-scroll::-webkit-scrollbar {
          display: none;
        }

        .hourly-scroll:focus-visible {
          outline: var(--cielo-focus-outline);
        }

        .hourly-list {
          display: grid;
          grid-auto-columns: 4.25rem;
          grid-auto-flow: column;
          width: max-content;
          padding: 0.8rem 0.45rem 0.75rem;
          margin: 0;
          list-style: none;
        }

        .hourly-period {
          display: grid;
          grid-template-rows: 1.1rem 2rem 1.25rem;
          gap: 0.35rem;
          min-width: 0;
          color: var(--cielo-color-text);
          place-items: center;
          scroll-snap-align: start;
        }

        .hourly-hour {
          color: var(--cielo-color-muted);
          font-size: var(--cielo-font-size-small);
          font-variant-numeric: tabular-nums;
          line-height: 1.1;
        }

        .hourly-period[data-current="true"] .hourly-hour {
          color: var(--cielo-color-text);
          font-weight: 650;
        }

        .hourly-condition-icon {
          width: 1.75rem;
          height: 1.75rem;
          align-self: center;
        }

        .hourly-condition-icon[hidden] {
          visibility: hidden;
        }

        .hourly-temperature {
          font-size: 1rem;
          font-variant-numeric: tabular-nums;
          font-weight: 560;
          line-height: 1.15;
        }

        @media (min-width: 48rem) {
          .header {
            padding-top: calc(1.25rem + env(safe-area-inset-top));
          }
        }

        @media (hover: hover) and (pointer: fine) {
          .locations-button:hover,
          .location-switcher:hover {
            background: var(--cielo-color-surface);
          }
        }

        /* Preserve the mobile reading order while fitting short landscape views. */
        @media (orientation: landscape) and (max-height: 34rem) {
          .screen {
            overflow-y: auto;
          }

          .header {
            display: flex;
            flex-direction: column;
            min-height: 100%;
            padding:
              calc(0.8rem + env(safe-area-inset-top))
              max(var(--cielo-space-4), env(safe-area-inset-right))
              max(0.8rem, env(safe-area-inset-bottom))
              max(var(--cielo-space-4), env(safe-area-inset-left));
          }

          .current-forecast {
            margin-top: 0;
          }

          .current-condition-icon {
            width: clamp(3.5rem, 18vh, 4.75rem);
            height: clamp(3.5rem, 18vh, 4.75rem);
          }

          #current-temperature-value {
            font-size: clamp(4.75rem, 26vh, 7rem);
          }

          .current-condition-description {
            font-size: clamp(0.9rem, 3.8vh, 1.05rem);
          }

          .current-forecast-message {
            padding: 0.9rem 0.5rem;
          }

          .hourly-forecast {
            position: static;
            width: calc(100% - 1.8rem);
            padding-top: 1rem;
            margin: auto auto 0;
            transform: none;
          }
        }

        @media (prefers-reduced-motion: reduce) {
          .screen {
            transition-duration: 1ms;
          }
        }
      </style>
      <section
        class="screen"
        tabindex="-1"
        aria-labelledby="municipality-title"
        aria-describedby="municipality-instructions"
      >
        <div class="back-swipe-region" aria-hidden="true"></div>
        <header class="header">
          <div class="header-content">
            <button
              id="locations-button"
              class="locations-button"
              type="button"
              aria-label="Ver ubicaciones"
            >
              <cielo-icon name="list"></cielo-icon>
            </button>
            <h1>
              <button
                id="municipality-switcher"
                class="location-switcher"
                type="button"
              >
                <span id="municipality-title" class="title-text"></span>
              </button>
            </h1>
          </div>
          <div class="current-forecast" role="status" aria-live="polite" aria-atomic="true">
            <span class="current-reading" aria-hidden="true">
              <span id="current-temperature-value">—</span>
              <cielo-icon
                id="current-condition-icon"
                class="current-condition-icon"
                hidden
              ></cielo-icon>
            </span>
            <span
              id="current-condition-description"
              class="current-condition-description"
              hidden
            ></span>
            <span id="current-temperature-announcement" class="visually-hidden">
              Temperatura actual no disponible
            </span>
            <span
              id="current-forecast-message"
              class="current-forecast-message"
            >
              Cargando previsión…
            </span>
          </div>
          <section id="hourly-forecast" class="hourly-forecast" hidden>
            <h2 id="hourly-forecast-title" class="visually-hidden">
              Previsión por horas
            </h2>
            <div
              class="hourly-scroll"
              tabindex="0"
              aria-labelledby="hourly-forecast-title"
            >
              <ul id="hourly-forecast-list" class="hourly-list"></ul>
            </div>
          </section>
          <p id="municipality-instructions" class="visually-hidden">
            Pulsa la lista o el nombre para elegir otra ubicación. También puedes deslizar hacia la derecha desde el borde izquierdo o usar Atrás.
          </p>
        </header>
      </section>
    `;
  }
}

/** @param {HourlyForecastPeriod} period @param {boolean} isCurrent */
function formatHourlyForecastLabel(period, isCurrent) {
  const hourLabel = isCurrent
    ? "Ahora"
    : `${period.hour} ${period.hour === 1 ? "hora" : "horas"}`;
  if (period.forecast === null) {
    return `${hourLabel}. Previsión no disponible`;
  }

  return `${hourLabel}. ${period.forecast.celsius} grados Celsius. ${period.forecast.description}`;
}

customElements.define("cielo-municipality-view", CieloMunicipalityView);
