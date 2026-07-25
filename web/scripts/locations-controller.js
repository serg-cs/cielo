import {
  getSavedMunicipalities,
  minimumSearchLength,
  normalizeSearchText,
  searchMunicipalities,
} from "./municipality-catalog.js";
import {
  MunicipalityRowGestureController,
} from "./municipality-row-gesture-controller.js";
import {
  requiredElement,
} from "./dom.js";

const maximumSearchResults = 50;
const maximumAutoScrollSpeed = 14;

export class LocationsController {
  #elements;
  #onOpen;
  #onRemove;
  #onReorder;
  #onRetry;
  #municipalities = [];
  #municipalitiesById = new Map();
  #savedMunicipalityIds = new Set();
  #currentConditionsById = new Map();
  #rowControllers = new Map();
  #ready = false;
  #searchActive = false;
  #reorder = null;
  #autoScrollFrame = null;

  constructor(root, { onOpen, onRemove, onReorder, onRetry }) {
    this.#elements = captureLocationsElements(root);
    this.#onOpen = onOpen;
    this.#onRemove = onRemove;
    this.#onReorder = onReorder;
    this.#onRetry = onRetry;
    this.#installInteractions();
  }

  set catalog(value) {
    this.#finishReorder(true, true);
    this.#municipalities = value;
    this.#municipalitiesById = new Map(
      value.map((municipality) => [municipality.id, municipality]),
    );
    this.#renderContent();
  }

  set savedMunicipalityIds(value) {
    this.#finishReorder(true, true);
    this.#savedMunicipalityIds = new Set(value);
    this.#renderContent();
  }

  showReady() {
    this.#ready = true;
    this.#elements.searchInput.disabled = false;
    this.#setCatalogStatus("");
    this.#renderContent();
    if (this.#elements.catalogRetryButton === document.activeElement) {
      this.#focusSavedContent();
    }
    this.#setCatalogRetryState(false);
  }

  showLoading() {
    this.#ready = false;
    this.#elements.searchInput.disabled = true;
    this.#setCatalogStatus("Cargando municipios…");
    const retryVisible = !this.#elements.catalogRetryButton.hidden;
    this.#setCatalogRetryState(retryVisible, retryVisible);
    this.#renderContent();
  }

  showError(message) {
    this.#ready = false;
    this.#elements.searchInput.disabled = true;
    this.#setCatalogStatus(message, true);
    this.#setCatalogRetryState(true);
    this.#renderContent();
  }

  clearSearch() {
    const activeElement = document.activeElement;
    const shouldRestoreFocus = activeElement === this.#elements.searchInput ||
      activeElement === this.#elements.clearSearchButton;
    this.#elements.searchInput.value = "";
    this.#elements.searchInput.blur();
    this.#searchActive = false;
    this.#renderContent();
    if (shouldRestoreFocus) {
      this.#focusSavedContent();
    }
  }

  restoreFocus(municipalityId) {
    const controller = this.#savedRowControllers.find(
      (candidate) => candidate.municipalityId === municipalityId,
    );
    if (controller !== undefined) {
      controller.focusOpenButton({ preventScroll: false });
      return;
    }

    this.#elements.searchInput.focus({ preventScroll: true });
  }

  focusAfterRemoval(removedIndex) {
    const controllers = this.#savedRowControllers;
    const adjacent = controllers[Math.min(removedIndex, controllers.length - 1)];
    if (adjacent !== undefined) {
      adjacent.focusOpenButton({ preventScroll: false });
      return;
    }

    this.#elements.searchInput.focus({ preventScroll: true });
  }

  closeSwipeRows(except = null) {
    for (const controller of this.#savedRowControllers) {
      if (controller !== except) {
        controller.closeAction();
      }
    }
  }

  setCurrentConditions(municipalityId, currentConditions) {
    if (currentConditions === null) {
      this.#currentConditionsById.delete(municipalityId);
    } else {
      this.#currentConditionsById.set(municipalityId, currentConditions);
    }

    for (const [key, controller] of this.#rowControllers) {
      if (key.endsWith(`:${municipalityId}`)) {
        const municipality = this.#municipalitiesById.get(municipalityId);
        if (municipality !== undefined) {
          controller.setCurrentConditions(municipality, currentConditions);
        }
      }
    }
  }

  setSourceUpdatedAt(generatedAt) {
    this.#elements.sourceUpdatedAt.dateTime = generatedAt;
    this.#elements.sourceUpdatedAt.textContent = generatedAt.slice(11, 16);
    this.#elements.sourceUpdate.hidden = false;
  }

  get #savedRowControllers() {
    return [
      ...this.#elements.savedList.querySelectorAll(
        ".cielo-municipality-row",
      ),
    ]
      .map((row) =>
        [...this.#rowControllers.values()]
          .find((controller) => controller.element === row)
      )
      .filter((controller) => controller !== undefined);
  }

  #installInteractions() {
    this.#elements.root.addEventListener("input", (event) => {
      if (event.target === this.#elements.searchInput) {
        this.#searchActive = true;
        this.#renderContent();
      }
    });
    this.#elements.root.addEventListener("focusin", (event) => {
      if (event.target === this.#elements.searchInput) {
        this.#searchActive = true;
        this.#renderContent();
        return;
      }

      this.closeSwipeRows(this.#rowControllerFromEvent(event));
    });
    this.#elements.root.addEventListener("keydown", (event) => {
      if (event.key === "Escape" && this.#reorder !== null) {
        event.preventDefault();
        this.#finishReorder(true, true);
        return;
      }
      if (
        event.target === this.#elements.searchInput &&
        event.key === "Escape"
      ) {
        event.preventDefault();
        this.clearSearch();
        return;
      }
      if (
        event.altKey &&
        (event.key === "ArrowUp" || event.key === "ArrowDown")
      ) {
        this.#handleKeyboardReorder(event);
      }
    });
    this.#elements.root.addEventListener("click", (event) => {
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
        this.#elements.searchInput.focus({ preventScroll: false });
        return;
      }
      if (event.target.closest("#catalog-retry") !== null) {
        this.#onRetry();
      }
    });
    this.#elements.root.addEventListener("scroll", () => {
      if (this.#reorder === null) {
        this.closeSwipeRows();
      }
    }, { passive: true });
    this.#elements.root.addEventListener("pointerdown", (event) => {
      this.closeSwipeRows(this.#rowControllerFromEvent(event));
    });
    this.#elements.root.addEventListener("pointermove", (event) => {
      this.#updateReorder(event);
    });
    this.#elements.root.addEventListener("pointerup", (event) => {
      if (this.#reorder?.pointerId === event.pointerId) {
        this.#finishReorder(false);
      }
    });
    this.#elements.root.addEventListener("pointercancel", (event) => {
      if (this.#reorder?.pointerId === event.pointerId) {
        this.#finishReorder(true);
      }
    });
    this.#elements.root.addEventListener("touchmove", (event) => {
      if (this.#reorder !== null) {
        event.preventDefault();
      }
    }, { passive: false });
    this.#elements.root.addEventListener("contextmenu", (event) => {
      if (this.#reorder !== null) {
        event.preventDefault();
      }
    });
  }

  #renderContent() {
    const query = normalizeSearchText(this.#elements.searchInput.value);
    this.#elements.savedSection.hidden = this.#searchActive;
    this.#elements.resultsSection.hidden = !this.#searchActive;
    this.#elements.clearSearchButton.hidden = !this.#searchActive;
    if (this.#searchActive) {
      this.closeSwipeRows();
      this.#renderResults(query);
    } else {
      this.#renderSaved();
    }
  }

  #renderSaved() {
    const municipalities = getSavedMunicipalities(
      this.#savedMunicipalityIds,
      this.#municipalitiesById,
    );

    this.#reconcileRows(this.#elements.savedList, municipalities, "saved");
    this.#elements.emptyGuidance.hidden =
      !this.#ready || municipalities.length > 0;
  }

  #renderResults(query) {
    if (!this.#ready || query.length < minimumSearchLength) {
      this.#reconcileRows(this.#elements.resultsList, [], "result");
      this.#setSearchStatus(
        this.#ready && query.length === 1
          ? "Escribe al menos 2 caracteres"
          : "",
      );
      this.#setSearchOverflowStatus("");
      return;
    }

    const municipalities = searchMunicipalities(this.#municipalities, query);
    this.#reconcileRows(
      this.#elements.resultsList,
      municipalities.slice(0, maximumSearchResults),
      "result",
    );
    this.#setSearchStatus(
      municipalities.length === 0 ? "No se encontraron municipios" : "",
    );
    this.#setSearchOverflowStatus(
      municipalities.length > maximumSearchResults
        ? `Mostrando ${maximumSearchResults} de ${municipalities.length} resultados`
        : "",
    );
  }

  #reconcileRows(list, municipalities, mode) {
    const desiredKeys = new Set(
      municipalities.map((municipality) => `${mode}:${municipality.id}`),
    );
    for (const [key, controller] of this.#rowControllers) {
      if (key.startsWith(`${mode}:`) && !desiredKeys.has(key)) {
        controller.disconnect();
        controller.element.remove();
        this.#rowControllers.delete(key);
      }
    }

    const elements = municipalities.map((municipality) => {
      const key = `${mode}:${municipality.id}`;
      let controller = this.#rowControllers.get(key);
      if (controller === undefined) {
        controller = this.#createRowController(municipality, mode);
        this.#rowControllers.set(key, controller);
      }
      controller.update({
        municipality,
        saved: mode === "saved",
        currentConditions:
          this.#currentConditionsById.get(municipality.id) ?? null,
      });
      return controller.element;
    });
    list.replaceChildren(...elements);
  }

  #createRowController(municipality, mode) {
    const fragment = this.#elements.municipalityRowTemplate.content.cloneNode(true);
    const element = fragment.firstElementChild;
    if (!(element instanceof HTMLElement)) {
      throw new Error("No se pudo crear la fila de municipio");
    }

    return new MunicipalityRowGestureController(element, {
      municipalityId: municipality.id,
      saved: mode === "saved",
      onOpen: (municipalityId) => {
        this.#onOpen({
          municipalityId,
          shouldSave: mode === "result" &&
            !this.#savedMunicipalityIds.has(municipalityId),
        });
      },
      onRemove: this.#onRemove,
      onReorderStart: (detail) => {
        this.#startReorder(detail);
      },
    });
  }

  #handleKeyboardReorder(event) {
    const controller = this.#rowControllerFromEvent(event);
    const controllers = this.#savedRowControllers;
    const sourceIndex = controller === null
      ? -1
      : controllers.indexOf(controller);
    if (sourceIndex === -1) {
      return;
    }

    const direction = event.key === "ArrowUp" ? -1 : 1;
    const targetIndex = sourceIndex + direction;
    if (targetIndex < 0 || targetIndex >= controllers.length) {
      return;
    }

    event.preventDefault();
    this.#onReorder({
      municipalityId: controller.municipalityId,
      targetIndex,
    });
    const municipality = this.#municipalitiesById.get(
      controller.municipalityId,
    );
    this.#announceReorder(
      municipality === undefined
        ? ""
        : `${municipality.name}, posición ${targetIndex + 1} de ${controllers.length}`,
    );
  }

  #startReorder(detail) {
    const controller = detail.controller;
    const row = controller.element;
    const list = this.#elements.savedList;
    const controllers = this.#savedRowControllers;
    if (
      controllers.length < 2 ||
      this.#reorder !== null ||
      row.parentElement !== list
    ) {
      controller.cancelReordering();
      return;
    }

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
    this.#elements.root.dataset.reordering = "true";
    this.#reorder = {
      ...detail,
      pointerY: detail.clientY,
      grabOffsetY: detail.clientY - rectangle.top,
      originalIndex: controllers.indexOf(controller),
      rowHeight: rectangle.height,
      row,
      list,
      placeholder,
    };
    this.#scheduleAutoScroll();
  }

  #updateReorder(event) {
    const reorder = this.#reorder;
    if (reorder === null || reorder.pointerId !== event.pointerId) {
      return;
    }

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

    const reference = this.#savedRowControllers
      .map((controller) => controller.element)
      .filter((row) => row !== reorder.row)
      .find((row) => {
        const rectangle = row.getBoundingClientRect();
        return reorder.pointerY < rectangle.top + rectangle.height / 2;
      });
    if (reference === undefined) {
      reorder.list.append(reorder.placeholder);
    } else {
      reorder.list.insertBefore(reorder.placeholder, reference);
    }
  }

  #finishReorder(cancelled, cancelRow = false) {
    const reorder = this.#reorder;
    if (reorder === null) {
      return;
    }

    this.#reorder = null;
    if (this.#autoScrollFrame !== null) {
      window.cancelAnimationFrame(this.#autoScrollFrame);
      this.#autoScrollFrame = null;
    }
    delete this.#elements.root.dataset.reordering;

    let targetIndex = reorder.originalIndex;
    if (cancelled) {
      reorder.placeholder.remove();
      const rows = this.#savedRowControllers
        .map((controller) => controller.element)
        .filter((row) => row !== reorder.row);
      reorder.list.insertBefore(
        reorder.row,
        rows[reorder.originalIndex] ?? null,
      );
    } else {
      reorder.list.insertBefore(reorder.row, reorder.placeholder);
      reorder.placeholder.remove();
      targetIndex = [...reorder.list.children].indexOf(reorder.row);
    }
    clearReorderStyles(reorder.row);
    if (cancelRow) {
      reorder.controller.cancelReordering();
    }

    if (!cancelled && targetIndex !== reorder.originalIndex) {
      this.#onReorder({
        municipalityId: reorder.municipalityId,
        targetIndex,
      });
    }
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

      const rectangle = this.#elements.root.getBoundingClientRect();
      const edgeSize = reorder.rowHeight;
      let speed = 0;
      if (reorder.pointerY < rectangle.top + edgeSize) {
        speed = -maximumAutoScrollSpeed *
          (1 - Math.max(0, reorder.pointerY - rectangle.top) / edgeSize);
      } else if (reorder.pointerY > rectangle.bottom - edgeSize) {
        speed = maximumAutoScrollSpeed *
          (1 - Math.max(0, rectangle.bottom - reorder.pointerY) / edgeSize);
      }
      if (speed === 0) {
        return;
      }

      const previousScrollTop = this.#elements.root.scrollTop;
      this.#elements.root.scrollTop += speed;
      if (this.#elements.root.scrollTop !== previousScrollTop) {
        this.#moveReorderPlaceholder();
        this.#scheduleAutoScroll();
      }
    });
  }

  #rowControllerFromEvent(event) {
    const row = event
      .composedPath()
      .find(
        (candidate) =>
          candidate instanceof HTMLElement &&
          candidate.matches(".cielo-municipality-row"),
      );
    if (!(row instanceof HTMLElement)) {
      return null;
    }

    return [...this.#rowControllers.values()]
      .find((controller) => controller.element === row) ?? null;
  }

  #setSearchStatus(message) {
    setStatus(this.#elements.searchStatus, message);
  }

  #setSearchOverflowStatus(message) {
    setStatus(this.#elements.searchOverflowStatus, message);
  }

  #setCatalogStatus(message, alert = false) {
    const status = this.#elements.catalogStatus;
    setStatus(status, message);
    status.setAttribute("role", alert ? "alert" : "status");
  }

  #setCatalogRetryState(visible, disabled = false) {
    const button = this.#elements.catalogRetryButton;
    button.hidden = !visible;
    if (disabled) {
      button.setAttribute("aria-disabled", "true");
    } else {
      button.removeAttribute("aria-disabled");
    }
  }

  #focusSavedContent() {
    const controller = this.#savedRowControllers[0];
    if (controller !== undefined) {
      controller.focusOpenButton({ preventScroll: true });
      return;
    }

    if (!this.#elements.emptySearchButton.hidden) {
      this.#elements.emptySearchButton.focus({ preventScroll: true });
      return;
    }

    this.#elements.searchInput.focus({ preventScroll: true });
  }

  #announceReorder(message) {
    const announcement = this.#elements.reorderAnnouncement;
    announcement.textContent = "";
    window.requestAnimationFrame(() => {
      announcement.textContent = message;
    });
  }
}

function captureLocationsElements(root) {
  return {
    root,
    searchInput: requiredElement(
      root.querySelector("#municipality-search"),
      HTMLInputElement,
    ),
    clearSearchButton: requiredElement(
      root.querySelector("#clear-search"),
      HTMLButtonElement,
    ),
    catalogStatus: requiredElement(
      root.querySelector("#catalog-status"),
      HTMLElement,
    ),
    catalogRetryButton: requiredElement(
      root.querySelector("#catalog-retry"),
      HTMLButtonElement,
    ),
    savedSection: requiredElement(
      root.querySelector("#saved-section"),
      HTMLElement,
    ),
    emptyGuidance: requiredElement(
      root.querySelector("#empty-guidance"),
      HTMLElement,
    ),
    emptySearchButton: requiredElement(
      root.querySelector("#empty-search"),
      HTMLButtonElement,
    ),
    savedList: requiredElement(
      root.querySelector("#saved-list"),
      HTMLElement,
    ),
    resultsSection: requiredElement(
      root.querySelector("#results-section"),
      HTMLElement,
    ),
    searchStatus: requiredElement(
      root.querySelector("#search-status"),
      HTMLElement,
    ),
    resultsList: requiredElement(
      root.querySelector("#results-list"),
      HTMLElement,
    ),
    searchOverflowStatus: requiredElement(
      root.querySelector("#search-overflow-status"),
      HTMLElement,
    ),
    sourceUpdate: requiredElement(
      root.querySelector("#source-update"),
      HTMLSpanElement,
    ),
    sourceUpdatedAt: requiredElement(
      root.querySelector("#source-updated-at"),
      HTMLTimeElement,
    ),
    reorderAnnouncement: requiredElement(
      root.querySelector("#reorder-announcement"),
      HTMLElement,
    ),
    municipalityRowTemplate: requiredElement(
      document.querySelector("#municipality-row-template"),
      HTMLTemplateElement,
    ),
  };
}

function setStatus(element, message) {
  element.textContent = message;
  element.hidden = message.length === 0;
}

function clearReorderStyles(row) {
  delete row.dataset.reorderActive;
  row.style.removeProperty("--reorder-left");
  row.style.removeProperty("--reorder-top");
  row.style.removeProperty("--reorder-width");
  row.style.removeProperty("--reorder-height");
}
