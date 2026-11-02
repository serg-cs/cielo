/**
 * Saved places (municipalities and beaches) and search.
 */

import { WARNING_LEVELS } from "./codes.js";
import { h, icon, preserveFocus, replaceChildren } from "./dom.js";
import { degrees } from "./format.js";
import { largestPlaces, search } from "./places.js";
import { placeHref } from "./router.js";

/** @typedef {import("./places.js").Place} Place */
/** @typedef {import("./places.js").Beach} Beach */

/**
 * @typedef {object} SavedSummary
 * @property {number | null} temp Current temperature, or the beach's water temperature.
 * @property {string | null} icon
 * @property {string} condition
 * @property {string | null} warning Highest warning level.
 */

const SUGGESTIONS = 6;

export class LocationsView {
  /**
   * @param {HTMLElement} root
   * @param {object} callbacks
   * @param {(ids: string[]) => void} callbacks.onReorder New order of the saved places.
   * @param {(id: string) => void} callbacks.onRemove
   * @param {(id: string) => void} callbacks.onPrefetch A place is about to be opened.
   */
  constructor(root, { onReorder, onRemove, onPrefetch }) {
    this.root = root;
    this.onReorder = onReorder;
    this.onRemove = onRemove;
    this.onPrefetch = onPrefetch;
    this.editing = false;
    this.query = "";
    /** What the list was last built from. */
    this.renderedKey = "";
    /** @type {string | null | undefined} */
    this.renderedUpdated = undefined;
    /** @type {{places: Map<string, Place>, beaches: Map<string, Beach>, saved: string[], summaries: Map<string, SavedSummary>, status: string | null, updated: string | null}} */
    this.state = {
      places: new Map(),
      beaches: new Map(),
      saved: [],
      summaries: new Map(),
      status: null,
      updated: null,
    };

    this.editButton = h("button.text-button", { type: "button", onclick: () => this.toggleEditing() }, "Editar");
    this.input = h("input.search-input", {
      id: "search",
      type: "search",
      autocomplete: "off",
      spellcheck: "false",
      enterkeyhint: "search",
      placeholder: "Municipio o playa",
      "aria-describedby": "search-status",
      oninput: () => {
        this.query = this.input.value;
        if (this.query.trim()) this.editing = false;
        this.renderList();
      },
      onkeydown: (/** @type {KeyboardEvent} */ event) => {
        if (event.key === "Escape") {
          // First clears the text, then leaves the field.
          if (this.input.value) this.clearSearch();
          else this.input.blur();
        } else if (event.key === "ArrowDown") {
          // From the keyboard, down goes into the results.
          const first = /** @type {HTMLAnchorElement | null} */ (this.list.querySelector(".results a"));
          if (first) {
            event.preventDefault();
            first.focus();
          }
        }
      },
    });
    this.clearButton = h(
      "button.icon-button.search-clear",
      {
        type: "button",
        "aria-label": "Borrar búsqueda",
        hidden: true,
        onclick: () => {
          this.clearSearch();
          this.input.focus();
        },
      },
      icon("circle-x", "icon icon-small"),
    );
    this.searchStatus = h("p.visually-hidden", {
      id: "search-status",
      role: "status",
    });
    this.list = h("div.locations-content");
    // Source and freshness at the bottom left, the guide's button at the
    // bottom right, both kept on screen.
    this.source = h("div.locations-source");
    this.footer = h(
      "footer.locations-bar",
      null,
      this.source,
      h(
        "button.guide-button",
        { type: "button", "data-guide": "", "aria-label": "Cómo leer la previsión" },
        icon("info", "icon"),
      ),
    );

    replaceChildren(
      root,
      h(
        "header.app-header",
        null,
        h("h1.app-title", null, icon("cloud-sun", "icon app-logo"), "Cielo"),
        this.editButton,
      ),
      h(
        "form.search",
        {
          role: "search",
          // Enter (the keyboard's search key) only hides the keyboard, so
          // the results can be seen and picked.
          onsubmit: (/** @type {Event} */ event) => {
            event.preventDefault();
            this.input.blur();
          },
        },
        h("label.visually-hidden", { for: "search" }, "Buscar municipio o playa"),
        icon("search", "icon search-icon"),
        this.input,
        this.clearButton,
      ),
      this.searchStatus,
      this.list,
      this.footer,
    );
  }

  clearSearch() {
    this.input.value = "";
    this.query = "";
    this.renderList();
  }

  /**
   * Close the search when the list is left: coming back shows the saved
   * places, not the old results.
   *
   * @returns {boolean} Whether a search was open.
   */
  closeSearch() {
    const open = this.query.trim().length > 0;
    this.input.blur();
    if (this.input.value || this.query) this.clearSearch();
    return open;
  }

  /**
   * @param {Partial<LocationsView["state"]>} state
   */
  update(state) {
    Object.assign(this.state, state);
    this.renderSource();
    // The list is only rebuilt when a row would change: rebuilding replaces
    // the row under the finger and loses the tap.
    if (this.listKey() === this.renderedKey) return;
    preserveFocus(this.list, () => this.renderList());
  }

  /** Everything the list shows that comes from outside the view. */
  listKey() {
    const { saved, summaries, status, places, beaches } = this.state;
    return JSON.stringify([saved, [...summaries], status, places.size, beaches.size]);
  }

  /** Source and freshness, in the bottom bar. */
  renderSource() {
    const { updated } = this.state;
    if (this.source.childElementCount > 0 && updated === this.renderedUpdated) return;
    this.renderedUpdated = updated;
    replaceChildren(
      this.source,
      h(
        "p",
        null,
        "Previsiones de ",
        h("a", { href: "https://www.aemet.es/", rel: "noopener", target: "_blank" }, "AEMET"),
      ),
      updated ? h("p", null, updated) : null,
    );
  }

  toggleEditing() {
    this.editing = !this.editing;
    this.renderList();
  }

  /**
   * Name and second line of a saved place.
   *
   * @param {string} id
   */
  describe(id) {
    const { places, beaches } = this.state;
    if (id.length === 7) {
      const beach = beaches.get(id);
      return {
        name: beach?.name ?? "Playa",
        // The parasol before the name already says it is a beach.
        where: beach ? beach.municipality : "",
        beach: true,
      };
    }
    const place = places.get(id);
    return {
      name: place?.name ?? "Municipio",
      where: place && place.province !== place.name ? place.province : "",
      beach: false,
    };
  }

  renderList() {
    const { saved, summaries, status } = this.state;
    const searching = this.query.trim().length > 0;
    this.clearButton.hidden = !searching;
    this.editButton.hidden = saved.length === 0 || searching;
    this.editButton.textContent = this.editing ? "Hecho" : "Editar";
    this.editButton.setAttribute("aria-pressed", String(this.editing));
    this.renderedKey = this.listKey();

    if (searching) {
      this.renderResults();
      return;
    }
    this.searchStatus.textContent = "";
    if (status && saved.length === 0) {
      replaceChildren(this.list, h("p.notice", { role: "status" }, status));
      return;
    }
    if (saved.length === 0) {
      this.renderWelcome();
      return;
    }

    const rows = saved.map((id, index) => {
      const { name, where, beach } = this.describe(id);
      const summary = summaries.get(id);
      const warning = summary?.warning ? WARNING_LEVELS[summary.warning] : null;
      const text = h(
        "span.place-text",
        null,
        h("span.place-name", null, beach ? icon("parasol", "icon icon-tiny place-kind") : null, name),
        where ? h("span.place-where", null, where) : null,
        // The warning is said in words under the name, like in the day list.
        warning && !this.editing
          ? h(
              "span.place-warning",
              { class: `place-warning level-${summary?.warning}` },
              h(
                "span.day-warning",
                { class: `day-warning level-${summary?.warning}` },
                icon("triangle-alert", "icon icon-tiny"),
              ),
              `Aviso ${warning.label.toLowerCase()}`,
            )
          : null,
      );
      if (this.editing) {
        return h(
          "li",
          null,
          h(
            "div.place-row.editing",
            null,
            text,
            h(
              "span.edit-actions",
              null,
              h(
                "button.icon-button",
                {
                  type: "button",
                  "aria-label": `Subir ${name}`,
                  disabled: index === 0,
                  onclick: () => this.move(index, -1, "up"),
                  dataset: { action: "up", id },
                },
                icon("move-up"),
              ),
              h(
                "button.icon-button",
                {
                  type: "button",
                  "aria-label": `Bajar ${name}`,
                  disabled: index === saved.length - 1,
                  onclick: () => this.move(index, 1, "down"),
                  dataset: { action: "down", id },
                },
                icon("move-down"),
              ),
              h(
                "button.icon-button.destructive",
                {
                  type: "button",
                  "aria-label": `Quitar ${name}`,
                  onclick: () => this.remove(id, index),
                  dataset: { action: "remove", id },
                },
                icon("trash-2"),
              ),
            ),
          ),
        );
      }
      const spoken = [
        name,
        where,
        summary?.temp != null ? `${beach ? "agua a " : ""}${summary.temp} grados, ${summary.condition}` : null,
        warning ? `aviso ${warning.label.toLowerCase()}` : null,
      ]
        .filter(Boolean)
        .join(", ");
      return h(
        "li",
        null,
        h(
          "a.place-row",
          {
            href: placeHref(id),
            "aria-label": spoken,
            onpointerdown: () => this.onPrefetch(id),
          },
          text,
          h(
            "span.place-side",
            null,
            summary?.icon ? icon(summary.icon, "icon icon-condition place-icon") : null,
            // Beaches show their water temperature in the same blue pill as
            // the beach rows of a municipality.
            beach
              ? summary?.temp != null
                ? h("span.water-chip", null, icon("waves", "icon icon-tiny"), degrees(summary.temp))
                : null
              : h("span.place-reading", null, h("span.place-temperature", null, degrees(summary?.temp))),
          ),
          icon("chevron-right", "icon icon-small link-chevron place-chevron"),
        ),
      );
    });
    replaceChildren(
      this.list,
      h("h2.section-title", { id: "saved-title" }, "Mis lugares"),
      h(
        "ul.place-list",
        {
          role: "list",
          "aria-labelledby": "saved-title",
          class: this.editing ? "place-list editing" : "place-list",
        },
        rows,
      ),
      this.editing ? h("p.notice", null, "Ordena tus lugares o quítalos de la lista.") : null,
    );
  }

  renderWelcome() {
    const suggestions = largestPlaces(this.state.places, SUGGESTIONS);
    replaceChildren(
      this.list,
      h(
        "div.welcome",
        null,
        icon("cloud-sun", "icon icon-hero welcome-icon"),
        h("h2", null, "El tiempo de AEMET, sencillo"),
        h("p", null, "Busca cualquier municipio o playa de España y guárdalo para verlo de un vistazo."),
      ),
      suggestions.length
        ? h(
            "section.suggestions",
            { "aria-labelledby": "suggestions-title" },
            h("h2.section-title", { id: "suggestions-title" }, "Para empezar"),
            h(
              "ul.chips",
              { role: "list" },
              suggestions.map((place) =>
                h(
                  "li",
                  null,
                  h(
                    "a.chip",
                    {
                      href: placeHref(place.id),
                      onpointerdown: () => this.onPrefetch(place.id),
                    },
                    icon("map-pin", "icon icon-tiny"),
                    place.name,
                  ),
                ),
              ),
            ),
          )
        : null,
    );
  }

  /**
   * @param {number} index
   * @param {number} offset
   * @param {string} action
   */
  move(index, offset, action) {
    const saved = [...this.state.saved];
    const target = index + offset;
    if (target < 0 || target >= saved.length) return;
    const [id] = saved.splice(index, 1);
    saved.splice(target, 0, id);
    this.onReorder(saved);
    this.searchStatus.textContent = `${this.describe(id).name} movido a la posición ${target + 1} de ${saved.length}`;
    // Keep focus on the same control for the moved row, or its sibling at the edges.
    const selector = `button[data-id="${id}"][data-action="${action}"]:not([disabled])`;
    const fallback = `button[data-id="${id}"]:not([disabled])`;
    /** @type {HTMLElement | null} */ (this.list.querySelector(selector) ?? this.list.querySelector(fallback))?.focus();
  }

  /**
   * @param {string} id
   * @param {number} index
   */
  remove(id, index) {
    const { name } = this.describe(id);
    this.onRemove(id);
    this.searchStatus.textContent = `${name} quitado`;
    if (this.state.saved.length === 0) {
      this.editing = false;
      this.renderList();
      this.input.focus();
      return;
    }
    const buttons = this.list.querySelectorAll("button.destructive");
    /** @type {HTMLElement | undefined} */ (buttons[Math.min(index, buttons.length - 1)])?.focus();
  }

  renderResults() {
    const { places, beaches, saved, status } = this.state;
    if (places.size === 0) {
      replaceChildren(this.list, h("p.notice", { role: "status" }, status ?? "Cargando lugares…"));
      return;
    }
    const { results, total } = search(places, this.query, beaches);
    if (results.length === 0) {
      this.searchStatus.textContent = "Sin resultados";
      replaceChildren(
        this.list,
        h("p.notice", null, `No hay municipios ni playas que coincidan con «${this.query.trim()}».`),
      );
      return;
    }
    this.searchStatus.textContent =
      total > results.length ? `Mostrando ${results.length} de ${total} resultados` : `${total} resultados`;
    replaceChildren(
      this.list,
      h(
        "ul.place-list.results",
        { role: "list", "aria-label": "Resultados" },
        results.map((place) => {
          const beach = place.kind === "beach";
          return h(
            "li",
            null,
            h(
              "a.place-row.result",
              {
                href: placeHref(place.id),
                onpointerdown: () => this.onPrefetch(place.id),
                onfocus: () => this.onPrefetch(place.id),
              },
              h(
                "span.result-kind",
                { class: beach ? "result-kind beach" : "result-kind" },
                icon(beach ? "parasol" : "map-pin", "icon icon-small"),
              ),
              h(
                "span.place-text",
                null,
                h("span.place-name", null, place.name),
                h(
                  "span.place-where",
                  null,
                  beach
                    ? `Playa · ${[/** @type {Beach} */ (place).municipality, place.province].filter((name, index, names) => names.indexOf(name) === index).join(", ")}`
                    : place.province !== place.name
                      ? place.province
                      : "",
                ),
              ),
              saved.includes(place.id)
                ? h(
                    "span.saved-mark",
                    null,
                    icon("check", "icon icon-small"),
                    h("span.visually-hidden", null, "guardado"),
                  )
                : null,
              icon("chevron-right", "icon icon-small link-chevron"),
            ),
          );
        }),
      ),
      total > results.length
        ? h("p.notice", null, `Mostrando ${results.length} de ${total}. Escribe más letras para afinar.`)
        : null,
    );
  }
}
