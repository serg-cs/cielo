import {
  getTrackedMunicipalities,
  minimumSearchLength,
  normalizeSearchText,
  searchMunicipalities,
} from "../lib/catalog.js";
import {
  CieloMunicipalityRow,
} from "./cielo-municipality-row.js";

const maximumSearchResults = 50;

/** @typedef {import("../lib/catalog.js").Municipality} Municipality */
/** @typedef {import("../lib/weather.js").CurrentForecast} CurrentForecast */

/**
 * @typedef {object} MunicipalityReorderStartDetail
 * @property {string} municipalityId
 * @property {number} pointerId
 * @property {number} clientY
 */

export class CieloLocationsView extends HTMLElement {
  #catalog = [];
  #municipalitiesById = new Map();
  #trackedIds = new Set();
  #currentForecasts = new Map();
  #ready = false;
  #searchActive = false;
  #reorder = null;
  #autoScrollFrame = null;

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.#renderShell();
    this.#installInteractions();
  }

  /** @param {Municipality[]} value */
  set catalog(value) {
    this.#finishReorder(true, true);
    this.#catalog = value;
    this.#municipalitiesById = new Map(
      value.map((municipality) => [municipality.id, municipality]),
    );
    this.#renderContent();
  }

  /** @param {Set<string>} value */
  set trackedIds(value) {
    this.#finishReorder(true, true);
    this.#trackedIds = new Set(value);
    this.#renderContent();
  }

  showReady() {
    this.#ready = true;
    const input = this.#searchInput;
    if (input !== null) {
      input.disabled = false;
    }
    this.#setLoadStatus("");
    this.#renderContent();
    if (this.#catalogRetryButton === this.shadowRoot?.activeElement) {
      this.#focusSavedContent();
    }
    this.#setCatalogRetryState(false);
  }

  showLoading() {
    this.#ready = false;
    const input = this.#searchInput;
    if (input !== null) {
      input.disabled = true;
    }
    this.#setLoadStatus("Cargando municipios…");
    const retryVisible = this.#catalogRetryButton?.hidden === false;
    this.#setCatalogRetryState(retryVisible, retryVisible);
    this.#renderContent();
  }

  /** @param {string} message */
  showError(message) {
    this.#ready = false;
    const input = this.#searchInput;
    if (input !== null) {
      input.disabled = true;
    }
    this.#setLoadStatus(message, true);
    this.#setCatalogRetryState(true);
    this.#renderContent();
  }

  clearSearch() {
    const input = this.#searchInput;
    const activeElement = this.shadowRoot?.activeElement;
    const shouldRestoreFocus = activeElement === input ||
      activeElement === this.#clearSearchButton;
    if (input !== null) {
      input.value = "";
      input.blur();
    }
    this.#searchActive = false;
    this.#renderContent();
    if (shouldRestoreFocus) {
      this.#focusSavedContent();
    }
  }

  /** @param {string} municipalityId */
  restoreFocus(municipalityId) {
    const row = this.#savedRows.find(
      (candidate) => candidate.municipality?.id === municipalityId,
    );
    if (row !== undefined) {
      row.focusOpenButton({ preventScroll: false });
      return;
    }

    this.#searchInput?.focus({ preventScroll: true });
  }

  /** @param {number} removedIndex */
  focusAfterRemoval(removedIndex) {
    const rows = this.#savedRows;
    const adjacentRow = rows[Math.min(removedIndex, rows.length - 1)];
    if (adjacentRow !== undefined) {
      adjacentRow.focusOpenButton({ preventScroll: false });
      return;
    }

    this.#searchInput?.focus({ preventScroll: true });
  }

  closeSwipeRows(except = null) {
    for (const row of this.#savedRows) {
      if (row !== except) {
        row.closeAction();
      }
    }
  }

  /** @param {string} municipalityId @param {CurrentForecast | null} forecast */
  setCurrentForecast(municipalityId, forecast) {
    if (forecast === null) {
      this.#currentForecasts.delete(municipalityId);
    } else {
      this.#currentForecasts.set(municipalityId, forecast);
    }

    // Update existing rows in place so live weather does not disturb interaction state.
    for (const row of this.shadowRoot?.querySelectorAll(
      "cielo-municipality-row",
    ) ?? []) {
      if (
        row instanceof CieloMunicipalityRow &&
        row.municipality?.id === municipalityId
      ) {
        row.currentForecast = forecast;
      }
    }
  }

  /** @returns {HTMLInputElement | null} */
  get #searchInput() {
    const input = this.shadowRoot?.querySelector("#municipality-search");
    return input instanceof HTMLInputElement ? input : null;
  }

  /** @returns {HTMLButtonElement | null} */
  get #clearSearchButton() {
    const button = this.shadowRoot?.querySelector("#clear-search");
    return button instanceof HTMLButtonElement ? button : null;
  }

  /** @returns {HTMLButtonElement | null} */
  get #catalogRetryButton() {
    const button = this.shadowRoot?.querySelector("#catalog-retry");
    return button instanceof HTMLButtonElement ? button : null;
  }

  /** @returns {CieloMunicipalityRow[]} */
  get #savedRows() {
    if (this.shadowRoot === null) {
      return [];
    }

    return [...this.shadowRoot.querySelectorAll("cielo-municipality-row[data-saved]")]
      .filter((row) => row instanceof CieloMunicipalityRow);
  }

  #installInteractions() {
    this.shadowRoot?.addEventListener("input", (event) => {
      if (event.target === this.#searchInput) {
        this.#searchActive = true;
        this.#renderContent();
      }
    });
    this.shadowRoot?.addEventListener("focusin", (event) => {
      if (event.target === this.#searchInput) {
        this.#searchActive = true;
        this.#renderContent();
        return;
      }

      this.closeSwipeRows(this.#rowFromEvent(event));
    });
    this.shadowRoot?.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && this.#reorder !== null) {
        event.preventDefault();
        this.#finishReorder(true, true);
        return;
      }

      if (event.target === this.#searchInput && event.key === "Escape") {
        event.preventDefault();
        this.clearSearch();
      }
    });
    this.shadowRoot?.addEventListener("click", (event) => {
      if (!(event.target instanceof Element)) {
        return;
      }

      if (event.target.closest("#clear-search") !== null) {
        this.clearSearch();
        return;
      }

      if (event.target.closest("#empty-search") !== null) {
        this.#searchActive = true;
        this.#renderContent();
        this.#searchInput?.focus({ preventScroll: false });
        return;
      }

      const catalogRetry = event.target.closest("#catalog-retry");
      if (
        catalogRetry instanceof HTMLButtonElement &&
        catalogRetry.getAttribute("aria-disabled") !== "true"
      ) {
        this.dispatchEvent(
          new CustomEvent("catalog-retry", {
            bubbles: true,
            composed: true,
          }),
        );
      }
    });
    this.addEventListener("scroll", () => {
      if (this.#reorder === null) {
        this.closeSwipeRows();
      }
    }, {
      passive: true,
    });
    this.shadowRoot?.addEventListener("pointerdown", (event) => {
      this.closeSwipeRows(this.#rowFromEvent(event));
    });
    this.shadowRoot?.addEventListener("municipality-reorder-start", (event) => {
      this.#startReorder(event);
    });
    this.shadowRoot?.addEventListener("pointermove", (event) => {
      this.#updateReorder(event);
    });
    this.shadowRoot?.addEventListener("pointerup", (event) => {
      if (this.#reorder?.pointerId === event.pointerId) {
        this.#finishReorder(false);
      }
    });
    this.shadowRoot?.addEventListener("pointercancel", (event) => {
      if (this.#reorder?.pointerId === event.pointerId) {
        this.#finishReorder(true);
      }
    });
    this.shadowRoot?.addEventListener("touchmove", (event) => {
      if (this.#reorder !== null) {
        event.preventDefault();
      }
    }, { passive: false });
    this.shadowRoot?.addEventListener("contextmenu", (event) => {
      if (this.#reorder !== null) {
        event.preventDefault();
      }
    });
  }

  /** @param {Event} event */
  #startReorder(event) {
    if (!(event instanceof CustomEvent)) {
      return;
    }

    event.stopPropagation();
    const detail = /** @type {MunicipalityReorderStartDetail} */ (event.detail);
    const row = this.#rowFromEvent(event);
    const list = this.shadowRoot?.querySelector("#saved-list");
    const rows = this.#savedRows;
    if (
      row === null ||
      !(list instanceof HTMLElement) ||
      row.municipality?.id !== detail.municipalityId ||
      rows.length < 2 ||
      this.#reorder !== null
    ) {
      row?.cancelReordering();
      return;
    }

    // Lift the active row while a placeholder preserves the grid geometry.
    this.closeSwipeRows();
    const rectangle = row.getBoundingClientRect();
    const placeholder = document.createElement("div");
    placeholder.className = "reorder-placeholder";
    placeholder.setAttribute("aria-hidden", "true");
    placeholder.style.height = `${rectangle.height}px`;
    list.insertBefore(placeholder, row);
    row.style.setProperty("--reorder-left", `${rectangle.left}px`);
    row.style.setProperty("--reorder-top", `${rectangle.top}px`);
    row.style.setProperty("--reorder-width", `${rectangle.width}px`);
    row.style.setProperty("--reorder-height", `${rectangle.height}px`);
    row.dataset.reorderActive = "true";
    this.dataset.reordering = "true";
    this.#reorder = {
      municipalityId: detail.municipalityId,
      pointerId: detail.pointerId,
      pointerY: detail.clientY,
      grabOffsetY: detail.clientY - rectangle.top,
      originalIndex: rows.indexOf(row),
      rowHeight: rectangle.height,
      row,
      list,
      placeholder,
    };
    this.#scheduleAutoScroll();
  }

  /** @param {PointerEvent} event */
  #updateReorder(event) {
    const reorder = this.#reorder;
    if (reorder === null || reorder.pointerId !== event.pointerId) {
      return;
    }

    // Keep the lifted row under the pointer and move its target slot live.
    event.preventDefault();
    reorder.pointerY = event.clientY;
    reorder.row.style.setProperty(
      "--reorder-top",
      `${event.clientY - reorder.grabOffsetY}px`,
    );
    this.#moveReorderPlaceholder();
    this.#scheduleAutoScroll();
  }

  #moveReorderPlaceholder() {
    const reorder = this.#reorder;
    if (reorder === null) {
      return;
    }

    const referenceRow = this.#savedRows
      .filter((row) => row !== reorder.row)
      .find((row) => {
        const rectangle = row.getBoundingClientRect();
        return reorder.pointerY < rectangle.top + rectangle.height / 2;
      });
    if (referenceRow === undefined) {
      reorder.list.append(reorder.placeholder);
    } else {
      reorder.list.insertBefore(reorder.placeholder, referenceRow);
    }
  }

  /** @param {boolean} cancelled @param {boolean} [cancelRow] */
  #finishReorder(cancelled, cancelRow = false) {
    const reorder = this.#reorder;
    if (reorder === null) {
      return;
    }

    // Stop scrolling before restoring the row to a stable list position.
    this.#reorder = null;
    if (this.#autoScrollFrame !== null) {
      window.cancelAnimationFrame(this.#autoScrollFrame);
      this.#autoScrollFrame = null;
    }
    delete this.dataset.reordering;

    let targetIndex = reorder.originalIndex;
    if (cancelled) {
      reorder.placeholder.remove();
      const remainingRows = this.#savedRows.filter((row) => row !== reorder.row);
      reorder.list.insertBefore(
        reorder.row,
        remainingRows[reorder.originalIndex] ?? null,
      );
    } else {
      reorder.list.insertBefore(reorder.row, reorder.placeholder);
      reorder.placeholder.remove();
      targetIndex = this.#savedRows.indexOf(reorder.row);
    }
    this.#clearReorderStyles(reorder.row);
    if (cancelRow) {
      reorder.row.cancelReordering();
    }

    if (!cancelled && targetIndex !== reorder.originalIndex) {
      this.dispatchEvent(
        new CustomEvent("municipality-reorder", {
          bubbles: true,
          composed: true,
          detail: {
            municipalityId: reorder.municipalityId,
            targetIndex,
          },
        }),
      );
    }
  }

  /** @param {CieloMunicipalityRow} row */
  #clearReorderStyles(row) {
    delete row.dataset.reorderActive;
    row.style.removeProperty("--reorder-left");
    row.style.removeProperty("--reorder-top");
    row.style.removeProperty("--reorder-width");
    row.style.removeProperty("--reorder-height");
  }

  #scheduleAutoScroll() {
    if (this.#reorder === null || this.#autoScrollFrame !== null) {
      return;
    }

    this.#autoScrollFrame = window.requestAnimationFrame(() => {
      this.#autoScrollFrame = null;
      const reorder = this.#reorder;
      if (reorder === null) {
        return;
      }

      const rectangle = this.getBoundingClientRect();
      const edgeSize = reorder.rowHeight;
      const maximumSpeed = 14;
      let speed = 0;
      if (reorder.pointerY < rectangle.top + edgeSize) {
        speed = -maximumSpeed *
          (1 - Math.max(0, reorder.pointerY - rectangle.top) / edgeSize);
      } else if (reorder.pointerY > rectangle.bottom - edgeSize) {
        speed = maximumSpeed *
          (1 - Math.max(0, rectangle.bottom - reorder.pointerY) / edgeSize);
      }

      if (speed === 0) {
        return;
      }

      const previousScrollTop = this.scrollTop;
      this.scrollTop += speed;
      if (this.scrollTop !== previousScrollTop) {
        this.#moveReorderPlaceholder();
        this.#scheduleAutoScroll();
      }
    });
  }

  /** @param {Event} event @returns {CieloMunicipalityRow | null} */
  #rowFromEvent(event) {
    const row = event
      .composedPath()
      .find((candidate) => candidate instanceof CieloMunicipalityRow);
    return row instanceof CieloMunicipalityRow ? row : null;
  }

  #renderShell() {
    if (this.shadowRoot === null) {
      return;
    }

    this.shadowRoot.innerHTML = `
      <style>
        :host {
          position: relative;
          display: block;
          height: 100%;
          overflow-y: auto;
          color-scheme: dark;
          color: var(--cielo-color-text);
          background: var(--cielo-color-locations-background);
          overscroll-behavior-y: contain;
          scrollbar-gutter: stable;
        }

        :host([data-reordering]) {
          cursor: grabbing;
          user-select: none;
          -webkit-user-select: none;
        }

        * {
          box-sizing: border-box;
        }

        [hidden] {
          display: none !important;
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

        .search-header {
          position: sticky;
          z-index: 5;
          top: 0;
          padding: calc(0.8rem + env(safe-area-inset-top)) var(--cielo-space-4) 0.8rem;
          background: var(--cielo-color-locations-background);
        }

        .search-row {
          display: flex;
          align-items: center;
          gap: var(--cielo-space-3);
          width: 100%;
          max-width: var(--cielo-content-width);
          margin-inline: auto;
        }

        .search-control {
          position: relative;
          flex: 1 1 auto;
          min-width: 0;
        }

        .search-icon {
          position: absolute;
          z-index: 1;
          top: 50%;
          left: 0.9rem;
          width: 1.15rem;
          height: 1.15rem;
          color: var(--cielo-color-muted);
          transform: translateY(-50%);
        }

        input {
          width: 100%;
          min-height: 3rem;
          padding: 0.7rem 3.35rem 0.7rem 2.65rem;
          border: 1px solid var(--cielo-color-border);
          border-radius: var(--cielo-radius-control);
          outline: none;
          outline-offset: 0.12rem;
          color: var(--cielo-color-text);
          background: transparent;
          font: inherit;
          font-size: 1rem;
          appearance: none;
          -webkit-appearance: none;
        }

        input::-webkit-search-cancel-button {
          display: none;
          -webkit-appearance: none;
        }

        input:focus-visible {
          outline: var(--cielo-focus-outline);
        }

        input::placeholder {
          color: var(--cielo-color-muted);
          font-size: 1rem;
          opacity: 1;
        }

        input:disabled {
          opacity: 0.58;
        }

        .clear-search-button {
          position: absolute;
          top: 50%;
          right: 0;
          display: grid;
          width: var(--cielo-touch-target);
          height: var(--cielo-touch-target);
          padding: 0;
          border: 0;
          border-radius: 50%;
          outline: none;
          color: var(--cielo-color-muted);
          background: transparent;
          cursor: pointer;
          place-items: center;
          transform: translateY(-50%);
          -webkit-tap-highlight-color: transparent;
        }

        .clear-search-button cielo-icon {
          width: 1.1rem;
          height: 1.1rem;
        }

        .clear-search-button:focus-visible,
        .empty-search-button:focus-visible,
        .catalog-retry-button:focus-visible {
          outline: var(--cielo-focus-outline);
        }

        .content {
          position: relative;
          z-index: 1;
          display: flex;
          flex-direction: column;
          max-width: var(--cielo-content-width);
          min-height: calc(100% - 4.6rem - env(safe-area-inset-top));
          margin-inline: auto;
          padding: 0.15rem var(--cielo-space-4) calc(2rem + env(safe-area-inset-bottom));
        }

        .status {
          min-height: 1.25rem;
          margin: 0;
          padding: 0.35rem 0 var(--cielo-space-3);
          color: var(--cielo-color-muted);
          font-size: var(--cielo-font-size-small);
          line-height: 1.4;
          text-align: center;
        }

        .catalog-retry-button {
          align-self: center;
          min-height: var(--cielo-touch-target);
          padding: 0.6rem 1rem;
          border: 1px solid var(--cielo-color-border);
          border-radius: var(--cielo-radius-pill);
          outline: none;
          outline-offset: 0.15rem;
          color: var(--cielo-color-text);
          background: transparent;
          font-weight: 650;
          cursor: pointer;
          -webkit-tap-highlight-color: transparent;
        }

        .catalog-retry-button[aria-disabled="true"] {
          cursor: wait;
          opacity: 0.58;
        }

        .list {
          display: grid;
          gap: var(--cielo-space-2);
          grid-template-columns: minmax(0, 1fr);
          background: transparent;
        }

        .reorder-placeholder {
          min-height: var(--cielo-row-height);
          border: 1px dashed rgb(252 252 250 / 28%);
          border-radius: 1rem;
          background: rgb(252 252 250 / 4%);
        }

        cielo-municipality-row[data-reorder-active="true"] {
          position: fixed;
          z-index: 20;
          top: var(--reorder-top);
          left: var(--reorder-left);
          width: var(--reorder-width);
          height: var(--reorder-height);
          border-radius: 1rem;
          box-shadow: 0 0.85rem 1.8rem rgb(13 34 50 / 34%);
          pointer-events: none;
          transform: scale(1.015);
          transform-origin: center;
          transition:
            box-shadow 120ms ease-out,
            transform 120ms ease-out;
        }

        .empty-state {
          display: grid;
          justify-items: center;
          gap: var(--cielo-space-3);
          min-height: 18rem;
          padding: clamp(2.5rem, 8vh, 4.5rem) 1.35rem;
          color: var(--cielo-color-muted);
          text-align: center;
          align-content: center;
        }

        .list:empty {
          display: none;
        }

        .empty-state-icon {
          width: 3.25rem;
          height: 3.25rem;
          padding: 0.8rem;
          border-radius: 50%;
          color: var(--cielo-color-accent);
          background: var(--cielo-color-surface);
        }

        .empty-state h2,
        .empty-state p {
          margin: 0;
        }

        .empty-state h2 {
          color: var(--cielo-color-text);
          font-size: 1.2rem;
          line-height: 1.25;
        }

        .empty-state p {
          max-width: 24rem;
          line-height: 1.5;
        }

        .empty-search-button {
          min-height: var(--cielo-touch-target);
          padding: 0.65rem 1.05rem;
          border: 1px solid rgb(252 252 250 / 26%);
          border-radius: var(--cielo-radius-pill);
          outline: none;
          outline-offset: 0.15rem;
          color: var(--cielo-color-locations-background);
          background: var(--cielo-color-accent);
          font-weight: 720;
          cursor: pointer;
          -webkit-tap-highlight-color: transparent;
        }

        .search-overflow-status {
          margin: 0;
          padding: 1rem 0 0.25rem;
          color: var(--cielo-color-muted);
          font-size: 0.8rem;
          line-height: 1.4;
          text-align: center;
        }

        .source-attribution {
          margin: auto 0 0;
          padding-top: 1.5rem;
          color: var(--cielo-color-muted);
          font-size: 0.75rem;
          line-height: 1.4;
          text-align: center;
        }

        button:active {
          opacity: 0.78;
        }

        @media (hover: hover) and (pointer: fine) {
          input:hover {
            border-color: rgb(252 252 250 / 30%);
          }

          .clear-search-button:hover {
            color: var(--cielo-color-text);
            background: var(--cielo-color-surface);
          }

          .empty-search-button:hover,
          .catalog-retry-button:hover {
            filter: brightness(1.08);
          }
        }

        @media (orientation: landscape) and (max-height: 34rem) {
          .empty-state {
            grid-template:
              "icon title action" auto
              "icon copy action" auto
              / auto minmax(0, 1fr) auto;
            min-height: 0;
            padding: 1.25rem 0.5rem;
            column-gap: var(--cielo-space-4);
            row-gap: var(--cielo-space-1);
            justify-items: start;
            text-align: left;
          }

          .empty-state-icon {
            grid-area: icon;
            align-self: center;
          }

          .empty-state h2 {
            grid-area: title;
          }

          .empty-state p {
            grid-area: copy;
          }

          .empty-search-button {
            grid-area: action;
            align-self: center;
          }
        }

        @media (min-width: 48rem) {
          .search-header {
            padding-top: calc(1.25rem + env(safe-area-inset-top));
          }

          .content {
            min-height: calc(100% - 5.05rem - env(safe-area-inset-top));
            padding-top: 0.55rem;
          }
        }

        @media (prefers-reduced-motion: reduce) {
          cielo-municipality-row[data-reorder-active="true"] {
            transition-duration: 1ms;
          }
        }
      </style>
      <h1 class="visually-hidden">Ubicaciones</h1>
      <header class="search-header">
        <div class="search-row">
          <label class="visually-hidden" for="municipality-search">Buscar municipio</label>
          <div class="search-control">
            <cielo-icon class="search-icon" name="search"></cielo-icon>
            <input
              id="municipality-search"
              type="search"
              autocomplete="off"
              autocapitalize="none"
              autocorrect="off"
              enterkeyhint="search"
              placeholder="Buscar municipio"
              spellcheck="false"
              aria-autocomplete="none"
              disabled
            >
            <button
              id="clear-search"
              class="clear-search-button"
              type="button"
              aria-label="Cerrar búsqueda"
              hidden
            >
              <cielo-icon name="circle-x"></cielo-icon>
            </button>
          </div>
        </div>
      </header>
      <div class="content">
        <p id="load-status" class="status" role="status">Cargando municipios…</p>
        <button
          id="catalog-retry"
          class="catalog-retry-button"
          type="button"
          hidden
        >
          Reintentar
        </button>
        <section id="saved-section" aria-label="Mis ubicaciones">
          <div id="empty-guidance" class="empty-state" hidden>
            <cielo-icon class="empty-state-icon" name="map-pin"></cielo-icon>
            <h2>Añade tu primera ubicación</h2>
            <p>
              Busca un municipio para consultarlo y tenerlo siempre a mano.
            </p>
            <button id="empty-search" class="empty-search-button" type="button">
              Buscar municipio
            </button>
          </div>
          <div id="saved-list" class="list" role="list" aria-label="Mis ubicaciones"></div>
        </section>
        <section id="results-section" aria-label="Resultados" hidden>
          <p id="search-status" class="status" role="status" hidden></p>
          <div id="results-list" class="list" role="list" aria-label="Resultados"></div>
          <p
            id="search-overflow-status"
            class="search-overflow-status"
            hidden
          ></p>
        </section>
        <p class="source-attribution">Fuente: AEMET</p>
      </div>
    `;
  }

  #renderContent() {
    if (this.shadowRoot === null) {
      return;
    }

    const query = normalizeSearchText(this.#searchInput?.value ?? "");
    const savedSection = this.shadowRoot.querySelector("#saved-section");
    const resultsSection = this.shadowRoot.querySelector("#results-section");
    const clearSearch = this.shadowRoot.querySelector("#clear-search");
    if (
      !(savedSection instanceof HTMLElement) ||
      !(resultsSection instanceof HTMLElement) ||
      !(clearSearch instanceof HTMLButtonElement)
    ) {
      return;
    }

    // Keep search mode explicit so clearing text does not dismiss it.
    savedSection.hidden = this.#searchActive;
    resultsSection.hidden = !this.#searchActive;
    clearSearch.hidden = !this.#searchActive;
    if (this.#searchActive) {
      this.closeSwipeRows();
      this.#renderResults(query);
    } else {
      this.#renderSaved();
    }
  }

  #renderSaved() {
    if (this.shadowRoot === null) {
      return;
    }

    const list = this.shadowRoot.querySelector("#saved-list");
    const guidance = this.shadowRoot.querySelector("#empty-guidance");
    if (!(list instanceof HTMLElement) || !(guidance instanceof HTMLElement)) {
      return;
    }

    const municipalities = getTrackedMunicipalities(
      this.#trackedIds,
      this.#municipalitiesById,
    );
    list.replaceChildren(
      ...municipalities.map((municipality) =>
        this.#createRow(municipality, "saved"),
      ),
    );
    guidance.hidden = !this.#ready || municipalities.length > 0;
  }

  /** @param {string} query */
  #renderResults(query) {
    if (this.shadowRoot === null) {
      return;
    }

    const list = this.shadowRoot.querySelector("#results-list");
    const status = this.shadowRoot.querySelector("#search-status");
    const overflowStatus = this.shadowRoot.querySelector(
      "#search-overflow-status",
    );
    if (
      !(list instanceof HTMLElement) ||
      !(status instanceof HTMLElement) ||
      !(overflowStatus instanceof HTMLElement)
    ) {
      return;
    }

    if (!this.#ready || query.length < minimumSearchLength) {
      list.replaceChildren();
      this.#setSearchStatus(
        this.#ready && query.length === 1
          ? "Escribe al menos 2 caracteres"
          : "",
      );
      this.#setSearchOverflowStatus("");
      return;
    }

    const municipalities = searchMunicipalities(this.#catalog, query);
    list.replaceChildren(
      ...municipalities.slice(0, maximumSearchResults).map((municipality) =>
        this.#createRow(municipality, "result"),
      ),
    );

    if (municipalities.length === 0) {
      this.#setSearchStatus("No se encontraron municipios");
    } else {
      this.#setSearchStatus("");
    }
    this.#setSearchOverflowStatus(
      municipalities.length > maximumSearchResults
        ? `Mostrando ${maximumSearchResults} de ${municipalities.length} resultados`
        : "",
    );
  }

  /** @param {Municipality} municipality @param {"saved" | "result"} mode */
  #createRow(municipality, mode) {
    const row = document.createElement("cielo-municipality-row");
    if (!(row instanceof CieloMunicipalityRow)) {
      throw new Error("No se pudo crear la fila de municipio");
    }

    row.mode = mode;
    row.tracked = this.#trackedIds.has(municipality.id);
    row.municipality = municipality;
    row.currentForecast = this.#currentForecasts.get(municipality.id) ?? null;
    if (mode === "saved") {
      row.dataset.saved = "";
    }
    return row;
  }

  /** @param {string} message */
  #setSearchStatus(message) {
    const status = this.shadowRoot?.querySelector("#search-status");
    if (!(status instanceof HTMLElement)) {
      return;
    }

    status.textContent = message;
    status.hidden = message.length === 0;
  }

  /** @param {string} message */
  #setSearchOverflowStatus(message) {
    const status = this.shadowRoot?.querySelector("#search-overflow-status");
    if (!(status instanceof HTMLElement)) {
      return;
    }

    status.textContent = message;
    status.hidden = message.length === 0;
  }

  #focusSavedContent() {
    const row = this.#savedRows[0];
    if (row !== undefined) {
      row.focusOpenButton({ preventScroll: true });
      return;
    }

    const emptySearch = this.shadowRoot?.querySelector("#empty-search");
    if (emptySearch instanceof HTMLButtonElement && !emptySearch.hidden) {
      emptySearch.focus({ preventScroll: true });
      return;
    }

    this.#searchInput?.focus({ preventScroll: true });
  }

  /** @param {boolean} visible @param {boolean} [disabled] */
  #setCatalogRetryState(visible, disabled = false) {
    const button = this.#catalogRetryButton;
    if (button === null) {
      return;
    }

    button.hidden = !visible;
    if (disabled) {
      button.setAttribute("aria-disabled", "true");
    } else {
      button.removeAttribute("aria-disabled");
    }
  }

  /** @param {string} message @param {boolean} [alert] */
  #setLoadStatus(message, alert = false) {
    const status = this.shadowRoot?.querySelector("#load-status");
    if (!(status instanceof HTMLElement)) {
      return;
    }

    status.textContent = message;
    status.hidden = message.length === 0;
    status.setAttribute("role", alert ? "alert" : "status");
  }
}

customElements.define("cielo-locations-view", CieloLocationsView);
