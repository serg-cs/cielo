import {
  compareMunicipalities,
  minimumSearchLength,
  normalizeSearchText,
  searchMunicipalities,
} from "../lib/catalog.js";
import {
  CieloMunicipalityRow,
} from "./cielo-municipality-row.js";

/** @typedef {import("../lib/catalog.js").Municipality} Municipality */

export class CieloLocationsView extends HTMLElement {
  #catalog = [];
  #trackedIds = new Set();
  #temperatures = new Map();
  #ready = false;
  #searchActive = false;

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.#renderShell();
    this.#installInteractions();
  }

  /** @param {Municipality[]} value */
  set catalog(value) {
    this.#catalog = value;
    this.#renderContent();
  }

  /** @param {Set<string>} value */
  set trackedIds(value) {
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
  }

  showError() {
    this.#ready = false;
    const input = this.#searchInput;
    if (input !== null) {
      input.disabled = true;
    }
    this.#setLoadStatus("No se pudieron cargar los municipios.", true);
    this.#renderContent();
  }

  clearSearch() {
    if (this.#searchInput !== null) {
      this.#searchInput.value = "";
      this.#searchInput.blur();
    }
    this.#searchActive = false;
    this.#renderContent();
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

  /** @param {string} municipalityId @param {number | null} celsius */
  setTemperature(municipalityId, celsius) {
    if (celsius === null) {
      this.#temperatures.delete(municipalityId);
    } else {
      this.#temperatures.set(municipalityId, celsius);
    }

    // Update existing rows in place so live weather does not disturb interaction state.
    for (const row of this.shadowRoot?.querySelectorAll(
      "cielo-municipality-row",
    ) ?? []) {
      if (
        row instanceof CieloMunicipalityRow &&
        row.municipality?.id === municipalityId
      ) {
        row.temperature = celsius;
      }
    }
  }

  /** @returns {HTMLInputElement | null} */
  get #searchInput() {
    const input = this.shadowRoot?.querySelector("#municipality-search");
    return input instanceof HTMLInputElement ? input : null;
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
      }
    });
    this.addEventListener("scroll", () => this.closeSwipeRows(), {
      passive: true,
    });
    this.shadowRoot?.addEventListener("pointerdown", (event) => {
      this.closeSwipeRows(this.#rowFromEvent(event));
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
          color-scheme: light;
          color: var(--cielo-color-text);
          background: var(--cielo-color-locations-background);
          overscroll-behavior-y: contain;
          scrollbar-gutter: stable;
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
          position: relative;
          z-index: 5;
          padding: calc(0.8rem + env(safe-area-inset-top)) var(--cielo-space-4) 0.8rem;
          background: transparent;
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
          padding: 0.7rem 2.8rem 0.7rem 2.65rem;
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
          right: 0.35rem;
          display: grid;
          width: 2.3rem;
          height: 2.3rem;
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
        .empty-search-button:focus-visible {
          outline: var(--cielo-focus-outline);
        }

        .content {
          position: relative;
          z-index: 1;
          max-width: var(--cielo-content-width);
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
        }

        .status[role="alert"] {
          color: #8e1730;
        }

        .list {
          display: grid;
          gap: var(--cielo-space-2);
          grid-template-columns: minmax(0, 1fr);
          background: transparent;
        }

        .empty-state {
          overflow: hidden;
          border: 1px solid var(--cielo-color-border);
          border-radius: var(--cielo-radius-control);
          background: transparent;
        }

        .list:empty {
          display: none;
        }

        .empty-state {
          display: grid;
          justify-items: center;
          gap: var(--cielo-space-3);
          min-height: 16rem;
          padding: 2.4rem 1.35rem;
          color: var(--cielo-color-muted);
          text-align: center;
          align-content: center;
        }

        .empty-state-icon {
          width: 3.4rem;
          height: 3.4rem;
          padding: 0.75rem;
          border: 1px solid rgb(0 0 0 / 14%);
          border-radius: 50%;
          color: #1746b3;
          background: rgb(255 255 255 / 24%);
        }

        .empty-state h3,
        .empty-state p {
          margin: 0;
        }

        .empty-state h3 {
          color: var(--cielo-color-text);
          font-size: 1.15rem;
          line-height: 1.25;
        }

        .empty-state p {
          max-width: 24rem;
          line-height: 1.5;
        }

        .empty-search-button {
          min-height: var(--cielo-touch-target);
          padding: 0.65rem 1.05rem;
          border: 1px solid rgb(0 0 0 / 14%);
          border-radius: var(--cielo-radius-pill);
          outline: none;
          outline-offset: 0.15rem;
          color: #fff;
          background: #315bd7;
          font-weight: 720;
          cursor: pointer;
          -webkit-tap-highlight-color: transparent;
        }

        button:active {
          opacity: 0.78;
        }

        @media (min-width: 48rem) {
          .search-header {
            padding-top: calc(1.25rem + env(safe-area-inset-top));
          }

          .content {
            padding-top: 0.55rem;
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
            >
              <cielo-icon name="circle-x"></cielo-icon>
            </button>
          </div>
        </div>
      </header>
      <div class="content">
        <p id="load-status" class="status" role="status">Cargando municipios…</p>
        <section id="saved-section" aria-label="Mis ubicaciones">
          <div id="empty-guidance" class="empty-state" hidden>
            <cielo-icon class="empty-state-icon" name="map-pin"></cielo-icon>
            <h3>Añade tu primera ubicación</h3>
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
        </section>
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
    if (
      !(savedSection instanceof HTMLElement) ||
      !(resultsSection instanceof HTMLElement)
    ) {
      return;
    }

    // Keep search mode explicit so clearing text does not dismiss it.
    savedSection.hidden = this.#searchActive;
    resultsSection.hidden = !this.#searchActive;
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

    const municipalities = this.#catalog.filter((municipality) =>
      this.#trackedIds.has(municipality.id),
    ).sort(compareMunicipalities);
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
    if (!(list instanceof HTMLElement) || !(status instanceof HTMLElement)) {
      return;
    }

    if (!this.#ready || query.length < minimumSearchLength) {
      list.replaceChildren();
      this.#setSearchStatus("");
      return;
    }

    const municipalities = searchMunicipalities(this.#catalog, query);
    list.replaceChildren(
      ...municipalities.map((municipality) =>
        this.#createRow(municipality, "result"),
      ),
    );

    if (municipalities.length === 0) {
      this.#setSearchStatus("No se encontraron municipios");
    } else {
      this.#setSearchStatus("");
    }
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
    row.temperature = this.#temperatures.get(municipality.id) ?? null;
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
