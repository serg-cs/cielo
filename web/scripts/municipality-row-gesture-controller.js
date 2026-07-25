import {
  requiredElement,
  setDynamicIcon,
} from "./dom.js";

const rowActionGap = 10;
const rowActionSize = 44;
const rowActionWidth = rowActionSize + rowActionGap * 2;
const rowGestureSlop = 8;
const rowLongPressDelay = 400;
const rowOpenVelocity = -0.45;

export class MunicipalityRowGestureController {
  #elements;
  #municipalityId;
  #saved;
  #onOpen;
  #onRemove;
  #onReorderStart;
  #gesture = {
    pointerId: null,
    startX: 0,
    startY: 0,
    startTime: 0,
    startOffset: 0,
    offset: 0,
    dragging: false,
    rejected: false,
    reordering: false,
    suppressClick: false,
    longPressTimeoutId: null,
  };

  constructor(
    element,
    {
      municipalityId,
      saved,
      onOpen,
      onRemove,
      onReorderStart,
    },
  ) {
    this.#elements = captureMunicipalityRowElements(element);
    this.#municipalityId = municipalityId;
    this.#saved = saved;
    this.#onOpen = onOpen;
    this.#onRemove = onRemove;
    this.#onReorderStart = onReorderStart;
    this.#installInteractions();
  }

  get element() {
    return this.#elements.root;
  }

  get municipalityId() {
    return this.#municipalityId;
  }

  get actionOpen() {
    return this.#elements.root.dataset.actionOpen === "true";
  }

  update({ municipality, saved, currentConditions }) {
    this.#municipalityId = municipality.id;
    this.#saved = saved;
    if (!saved) {
      this.closeAction();
    }
    this.#render(municipality, currentConditions);
  }

  setCurrentConditions(municipality, currentConditions) {
    this.#render(municipality, currentConditions);
  }

  closeAction() {
    this.#elements.root.dataset.actionOpen = "false";
    this.#setOffset(0, false);
  }

  focusOpenButton({ preventScroll = true } = {}) {
    this.#elements.openButton.focus({ preventScroll });
  }

  cancelReordering() {
    if (!this.#gesture.reordering) {
      return;
    }

    const button = this.#elements.openButton;
    const pointerId = this.#gesture.pointerId;
    if (
      pointerId !== null &&
      button.hasPointerCapture(pointerId)
    ) {
      button.releasePointerCapture(pointerId);
    }

    this.#cancelLongPress();
    this.#gesture.pointerId = null;
    this.#gesture.dragging = false;
    this.#gesture.rejected = false;
    this.#gesture.reordering = false;
    this.#gesture.suppressClick = true;
    this.#elements.root.dataset.reordering = "false";
  }

  disconnect() {
    this.#cancelLongPress();
  }

  #openAction() {
    if (!this.#saved) {
      return;
    }

    this.#elements.root.dataset.actionOpen = "true";
    this.#setOffset(-rowActionWidth, false);
  }

  #setOffset(offset, dragging) {
    this.#elements.root.style.setProperty("--row-offset", `${offset}px`);
    this.#elements.root.dataset.dragging = String(dragging);
  }

  #installInteractions() {
    this.#elements.root.addEventListener("pointerdown", (event) => {
      const button = this.#buttonFromEvent(event);
      if (
        button === null ||
        !this.#saved ||
        !event.isPrimary ||
        event.button !== 0
      ) {
        return;
      }

      this.#gesture.pointerId = event.pointerId;
      this.#gesture.startX = event.clientX;
      this.#gesture.startY = event.clientY;
      this.#gesture.startTime = event.timeStamp;
      this.#gesture.startOffset = this.actionOpen ? -rowActionWidth : 0;
      this.#gesture.offset = this.#gesture.startOffset;
      this.#gesture.dragging = false;
      this.#gesture.rejected = false;
      this.#gesture.reordering = false;
      this.#scheduleLongPress(button, event);
    });

    this.#elements.root.addEventListener("pointermove", (event) => {
      const button = this.#buttonFromEvent(event);
      if (
        button === null ||
        this.#gesture.pointerId !== event.pointerId ||
        this.#gesture.rejected
      ) {
        return;
      }
      if (this.#gesture.reordering) {
        event.preventDefault();
        return;
      }

      const horizontalDistance = event.clientX - this.#gesture.startX;
      const verticalDistance = event.clientY - this.#gesture.startY;
      if (!this.#gesture.dragging) {
        if (
          Math.abs(horizontalDistance) < rowGestureSlop &&
          Math.abs(verticalDistance) < rowGestureSlop
        ) {
          return;
        }

        this.#cancelLongPress();
        if (Math.abs(verticalDistance) >= Math.abs(horizontalDistance)) {
          this.#gesture.rejected = true;
          return;
        }

        this.#gesture.dragging = true;
        button.setPointerCapture(event.pointerId);
      }

      this.#gesture.offset = Math.max(
        -rowActionWidth,
        Math.min(0, this.#gesture.startOffset + horizontalDistance),
      );
      this.#setOffset(this.#gesture.offset, true);
      event.preventDefault();
    });

    this.#elements.root.addEventListener("pointerup", (event) => {
      this.#settleGesture(event);
    });
    this.#elements.root.addEventListener("pointercancel", (event) => {
      this.#settleGesture(event, true);
    });
    this.#elements.root.addEventListener("focusin", (event) => {
      if (event.target === this.#elements.removeButton) {
        this.#openAction();
      }
    });
    this.#elements.root.addEventListener("focusout", (event) => {
      if (
        !(event.relatedTarget instanceof Node) ||
        !this.#elements.root.contains(event.relatedTarget)
      ) {
        this.closeAction();
      }
    });
    this.#elements.root.addEventListener("click", (event) => {
      this.#handleClick(event);
    });
  }

  #scheduleLongPress(button, event) {
    this.#cancelLongPress();
    this.#gesture.longPressTimeoutId = window.setTimeout(() => {
      if (
        this.#gesture.pointerId !== event.pointerId ||
        this.#gesture.dragging ||
        this.#gesture.rejected ||
        !this.#elements.root.isConnected
      ) {
        return;
      }

      try {
        button.setPointerCapture(event.pointerId);
      } catch (error) {
        console.warn("No se pudo iniciar la reordenación", error);
        return;
      }

      this.closeAction();
      this.#gesture.reordering = true;
      this.#elements.root.dataset.reordering = "true";
      this.#onReorderStart({
        controller: this,
        municipalityId: this.#municipalityId,
        pointerId: event.pointerId,
        clientY: event.clientY,
      });
    }, rowLongPressDelay);
  }

  #cancelLongPress() {
    if (this.#gesture.longPressTimeoutId === null) {
      return;
    }

    window.clearTimeout(this.#gesture.longPressTimeoutId);
    this.#gesture.longPressTimeoutId = null;
  }

  #settleGesture(event, cancelled = false) {
    if (this.#gesture.pointerId !== event.pointerId) {
      return;
    }

    this.#cancelLongPress();
    const button = this.#elements.openButton;
    if (button.hasPointerCapture(event.pointerId)) {
      button.releasePointerCapture(event.pointerId);
    }

    if (this.#gesture.reordering) {
      this.#gesture.suppressClick = !cancelled;
      this.closeAction();
    } else if (this.#gesture.dragging && cancelled) {
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

    this.#gesture.pointerId = null;
    this.#gesture.dragging = false;
    this.#gesture.rejected = false;
    this.#gesture.reordering = false;
    this.#elements.root.dataset.reordering = "false";
  }

  #handleClick(event) {
    if (!(event.target instanceof Element)) {
      return;
    }
    if (this.#elements.removeButton.contains(event.target)) {
      this.#onRemove(this.#municipalityId);
      return;
    }
    if (!this.#elements.openButton.contains(event.target)) {
      return;
    }
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

    this.#onOpen(this.#municipalityId);
  }

  #buttonFromEvent(event) {
    if (!(event.target instanceof Element)) {
      return null;
    }

    return this.#elements.openButton.contains(event.target)
      ? this.#elements.openButton
      : null;
  }

  #render(municipality, currentConditions) {
    const { root, name, province, openButton, removeButton } = this.#elements;
    root.dataset.mode = this.#saved ? "saved" : "result";
    root.dataset.municipalityId = municipality.id;
    name.textContent = municipality.name;
    province.textContent = municipality.province;
    openButton.classList.toggle("saved", this.#saved);
    openButton.classList.toggle("result", !this.#saved);
    openButton.setAttribute(
      "aria-label",
      rowActionLabel(municipality, this.#saved, currentConditions),
    );
    if (this.#saved) {
      openButton.setAttribute(
        "aria-keyshortcuts",
        "Alt+ArrowUp Alt+ArrowDown",
      );
    } else {
      openButton.removeAttribute("aria-keyshortcuts");
    }
    removeButton.setAttribute(
      "aria-label",
      `Eliminar ${municipality.name}, ${municipality.province}`,
    );
    this.#elements.temperature.textContent = currentConditions === null
      ? "—"
      : `${currentConditions.temperatureCelsius}°`;
    setDynamicIcon(
      this.#elements.conditionIcon,
      currentConditions?.condition ?? null,
    );
  }
}

function captureMunicipalityRowElements(root) {
  return {
    root,
    name: requiredElement(
      root.querySelector(".municipality-name"),
      HTMLElement,
    ),
    province: requiredElement(
      root.querySelector(".municipality-province"),
      HTMLElement,
    ),
    openButton: requiredElement(
      root.querySelector(".open-button"),
      HTMLButtonElement,
    ),
    removeButton: requiredElement(
      root.querySelector(".remove-button"),
      HTMLButtonElement,
    ),
    temperature: requiredElement(
      root.querySelector(".temperature"),
      HTMLElement,
    ),
    conditionIcon: requiredElement(
      root.querySelector(".condition-icon"),
      SVGElement,
    ),
  };
}

function rowActionLabel(municipality, saved, currentConditions) {
  const action = saved
    ? `Abrir ${municipality.name}, ${municipality.province}`
    : `Guardar y abrir ${municipality.name}, ${municipality.province}`;
  if (!saved || currentConditions === null) {
    return action;
  }

  return `${action}. Temperatura actual: ${currentConditions.temperatureCelsius} grados Celsius`;
}
