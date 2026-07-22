const edgeSwipeWidth = 28;
const gestureSlop = 8;
const screenDismissDistance = 96;
const screenDismissVelocity = 0.5;

/** @typedef {import("../lib/catalog.js").Municipality} Municipality */

export class CieloMunicipalityView extends HTMLElement {
  #municipality = null;
  #temperature = null;
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

  /** @param {Municipality} municipality @param {number | null} temperature */
  show(municipality, temperature) {
    const screen = this.#screen;
    const title = this.#title;
    const titleText = this.#titleText;
    if (screen === null || title === null || titleText === null) {
      return;
    }

    // Replace any superseded edge transition before showing the next screen.
    this.#cancelPendingTransition();
    this.#municipality = municipality;
    this.#temperature = temperature;
    this.#hiding = false;
    this.#edgeDismiss = false;
    this.#transitionToken += 1;
    titleText.textContent = municipality.name;
    title.setAttribute(
      "aria-label",
      `Cambiar ubicación. Ubicación actual: ${municipality.name}, ${municipality.province}`,
    );
    this.#renderTemperature();
    this.hidden = false;
    screen.dataset.dragging = "false";
    screen.dataset.edgeDismiss = "false";
    this.#installDocumentKeys();

    // Present route changes immediately while resetting interactive offset.
    screen.style.setProperty("--screen-offset-x", "0px");
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
    this.#temperature = null;
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

  /** @param {string} municipalityId @param {number | null} temperature */
  setTemperature(municipalityId, temperature) {
    if (this.#municipality?.id !== municipalityId) {
      return;
    }

    this.#temperature = temperature;
    this.#renderTemperature();
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

  #renderTemperature() {
    const value = this.shadowRoot?.querySelector("#current-temperature-value");
    const announcement = this.shadowRoot?.querySelector(
      "#current-temperature-announcement",
    );
    if (!(value instanceof HTMLElement) || !(announcement instanceof HTMLElement)) {
      return;
    }

    value.textContent = this.#temperature === null
      ? "—"
      : `${this.#temperature}°`;
    announcement.textContent = this.#temperature === null
      ? "Temperatura actual no disponible"
      : `Temperatura actual: ${this.#temperature} grados Celsius`;
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
          color-scheme: light;
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
          overflow-y: auto;
          outline: none;
          color: var(--cielo-color-text);
          background: var(--cielo-color-municipality-background);
          overscroll-behavior-y: contain;
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

        .current-temperature {
          margin: 0.55rem 0 0 0.9rem;
          font-size: clamp(6rem, 30vw, 10rem);
          font-variant-numeric: tabular-nums;
          font-weight: 300;
          letter-spacing: -0.075em;
          line-height: 0.9;
        }

        @media (min-width: 48rem) {
          .header {
            padding-top: calc(1.25rem + env(safe-area-inset-top));
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
          <p class="current-temperature" role="status" aria-live="polite" aria-atomic="true">
            <span id="current-temperature-value" aria-hidden="true">—</span>
            <span id="current-temperature-announcement" class="visually-hidden">
              Temperatura actual no disponible
            </span>
          </p>
          <p id="municipality-instructions" class="visually-hidden">
            Pulsa la lista o el nombre para elegir otra ubicación. También puedes deslizar hacia la derecha desde el borde izquierdo o usar Atrás.
          </p>
        </header>
      </section>
    `;
  }
}

customElements.define("cielo-municipality-view", CieloMunicipalityView);
