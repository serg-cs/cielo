const rowActionGap = 10;
const rowActionSize = 44;
const rowActionWidth = rowActionSize + rowActionGap * 2;
const rowGestureSlop = 8;
const rowOpenVelocity = -0.45;

/**
 * A municipality record supplied through the `municipality` property.
 * @typedef {import("../lib/catalog.js").Municipality} Municipality
 */

export class CieloMunicipalityRow extends HTMLElement {
  #municipality = null;
  #mode = "saved";
  #tracked = false;
  #temperature = null;
  #gesture = {
    pointerId: null,
    startX: 0,
    startY: 0,
    startTime: 0,
    startOffset: 0,
    offset: 0,
    dragging: false,
    rejected: false,
    suppressClick: false,
  };

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.#installInteractions();
  }

  connectedCallback() {
    this.setAttribute("role", "listitem");
  }

  /** @param {Municipality | null} value */
  set municipality(value) {
    this.#municipality = value;
    this.#render();
  }

  /** @returns {Municipality | null} */
  get municipality() {
    return this.#municipality;
  }

  /** @param {"saved" | "result"} value */
  set mode(value) {
    this.#mode = value;
    this.#render();
  }

  /** @param {boolean} value */
  set tracked(value) {
    this.#tracked = value;
    this.#render();
  }

  /** @param {number | null} value */
  set temperature(value) {
    this.#temperature = value;
    this.#renderTemperature();
  }

  get actionOpen() {
    return this.dataset.actionOpen === "true";
  }

  closeAction() {
    this.dataset.actionOpen = "false";
    this.#setOffset(0, false);
  }

  /** @param {{preventScroll?: boolean}} [options] */
  focusOpenButton({ preventScroll = true } = {}) {
    this.shadowRoot?.querySelector(".open-button")?.focus({ preventScroll });
  }

  #openAction() {
    if (this.#mode !== "saved") {
      return;
    }

    this.dataset.actionOpen = "true";
    this.#setOffset(-rowActionWidth, false);
  }

  /** @param {number} offset @param {boolean} dragging */
  #setOffset(offset, dragging) {
    this.style.setProperty("--row-offset", `${offset}px`);
    this.dataset.dragging = String(dragging);
  }

  #installInteractions() {
    this.shadowRoot?.addEventListener("pointerdown", (event) => {
      // Qualify primary-button drags that start on the row's open control.
      const button = this.#getOpenButton(event);
      if (
        button === null ||
        this.#mode !== "saved" ||
        !event.isPrimary ||
        event.button !== 0
      ) {
        return;
      }

      // Record the current reveal position as the gesture origin.
      this.#gesture.pointerId = event.pointerId;
      this.#gesture.startX = event.clientX;
      this.#gesture.startY = event.clientY;
      this.#gesture.startTime = event.timeStamp;
      this.#gesture.startOffset = this.actionOpen ? -rowActionWidth : 0;
      this.#gesture.offset = this.#gesture.startOffset;
      this.#gesture.dragging = false;
      this.#gesture.rejected = false;
    });

    this.shadowRoot?.addEventListener("pointermove", (event) => {
      // Continue only the active pointer sequence on the open control.
      const button = this.#getOpenButton(event);
      if (
        button === null ||
        this.#gesture.pointerId !== event.pointerId ||
        this.#gesture.rejected
      ) {
        return;
      }

      // Wait through slop, then reject movement intended for vertical scrolling.
      const horizontalDistance = event.clientX - this.#gesture.startX;
      const verticalDistance = event.clientY - this.#gesture.startY;
      if (!this.#gesture.dragging) {
        if (
          Math.abs(horizontalDistance) < rowGestureSlop &&
          Math.abs(verticalDistance) < rowGestureSlop
        ) {
          return;
        }

        if (Math.abs(verticalDistance) >= Math.abs(horizontalDistance)) {
          this.#gesture.rejected = true;
          return;
        }

        // Capture only after horizontal drag intent is established.
        this.#gesture.dragging = true;
        button.setPointerCapture(event.pointerId);
      }

      // Reveal the destructive action in direct proportion to the drag.
      this.#gesture.offset = Math.max(
        -rowActionWidth,
        Math.min(0, this.#gesture.startOffset + horizontalDistance),
      );
      this.#setOffset(this.#gesture.offset, true);
      event.preventDefault();
    });

    // Settle completed or cancelled drags at a stable reveal position.
    this.shadowRoot?.addEventListener("pointerup", (event) => {
      this.#settleGesture(event);
    });
    this.shadowRoot?.addEventListener("pointercancel", (event) => {
      this.#settleGesture(event, true);
    });

    // Keep the destructive action exposed while keyboard focus is inside it.
    this.shadowRoot?.addEventListener("focusin", (event) => {
      if (event.target instanceof HTMLElement && event.target.matches(".remove-button")) {
        this.#openAction();
      }
    });
    this.shadowRoot?.addEventListener("focusout", (event) => {
      if (
        !(event.relatedTarget instanceof Node) ||
        !this.shadowRoot?.contains(event.relatedTarget)
      ) {
        this.closeAction();
      }
    });

    // Coordinate action dispatch and synthetic-click suppression in one place.
    this.shadowRoot?.addEventListener("click", (event) => {
      this.#handleClick(event);
    });
  }

  /** @param {PointerEvent} event @param {boolean} [cancelled] */
  #settleGesture(event, cancelled = false) {
    if (this.#gesture.pointerId !== event.pointerId) {
      return;
    }

    // Release capture before choosing the gesture's final resting state.
    const button = this.shadowRoot?.querySelector(".open-button");
    if (button instanceof HTMLButtonElement && button.hasPointerCapture(event.pointerId)) {
      button.releasePointerCapture(event.pointerId);
    }

    // Restore cancelled drags or settle completed drags at the nearest state.
    if (this.#gesture.dragging && cancelled) {
      this.#gesture.suppressClick = false;
      if (this.#gesture.startOffset === -rowActionWidth) {
        this.#openAction();
      } else {
        this.closeAction();
      }
    } else if (this.#gesture.dragging) {
      this.#gesture.suppressClick = true;
      const duration = Math.max(1, event.timeStamp - this.#gesture.startTime);
      const velocity = (event.clientX - this.#gesture.startX) / duration;
      const flickedOpen = this.#gesture.offset < this.#gesture.startOffset &&
        velocity <= rowOpenVelocity;
      if (this.#gesture.offset <= -rowActionWidth / 2 || flickedOpen) {
        this.#openAction();
      } else {
        this.closeAction();
      }
    }

    // Reset pointer qualification for the next gesture.
    this.#gesture.pointerId = null;
    this.#gesture.dragging = false;
    this.#gesture.rejected = false;
  }

  /** @param {Event} event */
  #handleClick(event) {
    if (!(event.target instanceof Element) || this.#municipality === null) {
      return;
    }

    if (event.target.closest(".remove-button") !== null) {
      this.dispatchEvent(
        new CustomEvent("municipality-remove", {
          bubbles: true,
          composed: true,
          detail: { municipalityId: this.#municipality.id },
        }),
      );
      return;
    }

    if (event.target.closest(".open-button") === null) {
      return;
    }

    // Consume the synthetic click emitted after a completed pointer drag.
    if (this.#gesture.suppressClick) {
      this.#gesture.suppressClick = false;
      event.preventDefault();
      return;
    }

    if (this.actionOpen) {
      this.closeAction();
      event.preventDefault();
      return;
    }

    this.dispatchEvent(
      new CustomEvent("municipality-open", {
        bubbles: true,
        composed: true,
        detail: {
          municipalityId: this.#municipality.id,
          shouldTrack: this.#mode === "result" && !this.#tracked,
        },
      }),
    );
  }

  /** @param {Event} event @returns {HTMLButtonElement | null} */
  #getOpenButton(event) {
    if (!(event.target instanceof Element)) {
      return null;
    }

    const button = event.target.closest(".open-button");
    return button instanceof HTMLButtonElement ? button : null;
  }

  #render() {
    if (this.shadowRoot === null || this.#municipality === null) {
      return;
    }

    const municipality = this.#municipality;
    const isSavedRow = this.#mode === "saved";
    const actionLabel = this.#actionLabel;

    this.dataset.actionOpen = "false";
    this.dataset.dragging = "false";
    this.style.setProperty("--row-offset", "0px");
    this.shadowRoot.innerHTML = `
      <style>
        :host {
          display: block;
          width: 100%;
          min-width: 0;
          max-width: 100%;
          min-height: var(--cielo-row-height);
          --row-action-gap: ${rowActionGap}px;
          --row-action-size: ${rowActionSize}px;
          --row-action-width: ${rowActionWidth}px;
        }

        * {
          box-sizing: border-box;
        }

        .row {
          position: relative;
          min-height: var(--cielo-row-height);
          overflow: hidden;
          background: transparent;
        }

        button {
          color: inherit;
          font: inherit;
          cursor: pointer;
        }

        .open-button {
          position: relative;
          z-index: 1;
          display: flex;
          align-items: center;
          width: 100%;
          min-height: var(--cielo-row-height);
          padding: 0.6rem 1rem;
          border: 1px solid var(--cielo-color-border);
          border-radius: 1rem;
          outline: none;
          outline-offset: -0.2rem;
          color: var(--cielo-color-text);
          background: transparent;
          text-align: left;
        }

        .open-button:focus-visible {
          outline: var(--cielo-focus-outline);
        }

        .open-button.saved {
          transform: translateX(var(--row-offset, 0));
          transition: transform 280ms cubic-bezier(0.22, 1, 0.36, 1);
          touch-action: pan-y;
        }

        :host([data-dragging="true"]) .open-button.saved {
          transition: none;
        }

        .summary {
          flex: 1 1 auto;
          min-width: 0;
          overflow: hidden;
        }

        .temperature {
          flex: 0 0 auto;
          margin-left: var(--cielo-space-3);
          font-size: clamp(1.65rem, 7vw, 2rem);
          font-variant-numeric: tabular-nums;
          font-weight: 540;
          letter-spacing: -0.045em;
          line-height: 1;
        }

        .name,
        .province {
          display: block;
          overflow: hidden;
          text-overflow: ellipsis;
          white-space: nowrap;
        }

        .name {
          font-size: var(--cielo-font-size-title);
          font-weight: 660;
          line-height: 1.25;
        }

        .province {
          margin-top: 0.12rem;
          color: var(--cielo-color-muted);
          font-size: var(--cielo-font-size-small);
        }

        .remove-button {
          position: absolute;
          z-index: 0;
          top: 50%;
          right: calc(var(--row-action-gap) - var(--row-action-width));
          display: grid;
          width: var(--row-action-size);
          height: var(--row-action-size);
          padding: 0;
          border: 1px solid rgb(255 255 255 / 28%);
          border-radius: 50%;
          outline: none;
          outline-offset: 0.12rem;
          color: #fff;
          background: var(--cielo-color-destructive);
          box-shadow: 0 0.2rem 0.45rem rgb(50 7 14 / 24%);
          opacity: 0;
          place-items: center;
          transform:
            translateY(-50%)
            translateX(var(--row-offset, 0))
            scale(0.82);
          transition:
            opacity 140ms ease-out,
            transform 240ms cubic-bezier(0.22, 1, 0.36, 1);
        }

        :host([data-action-open="true"]) .remove-button {
          opacity: 1;
          transform:
            translateY(-50%)
            translateX(var(--row-offset, 0))
            scale(1);
        }

        :host([data-dragging="true"]) .remove-button {
          opacity: 1;
          transform:
            translateY(-50%)
            translateX(var(--row-offset, 0))
            scale(1);
          transition: none;
        }

        .remove-button:focus-visible {
          outline: var(--cielo-focus-outline);
        }

        .remove-button cielo-icon {
          width: 1.3rem;
          height: 1.3rem;
        }

        @media (prefers-reduced-motion: reduce) {
          .open-button.saved,
          .remove-button {
            transition-duration: 1ms;
          }
        }
      </style>
      <div class="row">
        <button
          class="open-button ${isSavedRow ? "saved" : "result"}"
          type="button"
          aria-label="${escapeAttribute(isSavedRow ? `Abrir ${municipality.name}, ${municipality.province}` : actionLabel)}"
        >
          <span class="summary">
            <span class="name"></span>
            <span class="province"></span>
          </span>
          ${isSavedRow ? '<span class="temperature" aria-hidden="true">—</span>' : ""}
        </button>
        ${
          isSavedRow
            ? `<button class="remove-button" type="button" title="Eliminar" aria-label="${escapeAttribute(`Eliminar ${municipality.name}, ${municipality.province}`)}">
                <cielo-icon name="trash-2"></cielo-icon>
              </button>`
            : ""
        }
      </div>
    `;

    const name = this.shadowRoot.querySelector(".name");
    const province = this.shadowRoot.querySelector(".province");
    if (name !== null && province !== null) {
      name.textContent = municipality.name;
      province.textContent = municipality.province;
    }
    this.#renderTemperature();
  }

  #renderTemperature() {
    if (this.shadowRoot === null || this.#municipality === null) {
      return;
    }

    const temperature = this.shadowRoot.querySelector(".temperature");
    if (temperature !== null) {
      temperature.textContent = formatTemperature(this.#temperature);
    }
    this.shadowRoot
      .querySelector(".open-button")
      ?.setAttribute("aria-label", this.#actionLabel);
  }

  get #actionLabel() {
    if (this.#municipality === null) {
      return "";
    }

    const municipality = this.#municipality;
    const action = this.#tracked
      ? `Abrir ${municipality.name}, ${municipality.province}`
      : `Guardar y abrir ${municipality.name}, ${municipality.province}`;
    if (this.#mode !== "saved" || this.#temperature === null) {
      return action;
    }

    return `${action}. Temperatura actual: ${this.#temperature} grados Celsius`;
  }
}

/** @param {number | null} celsius */
function formatTemperature(celsius) {
  return celsius === null ? "—" : `${celsius}°`;
}

/** @param {string} value */
function escapeAttribute(value) {
  return value
    .replaceAll("&", "&amp;")
    .replaceAll('"', "&quot;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;");
}

customElements.define("cielo-municipality-row", CieloMunicipalityRow);
