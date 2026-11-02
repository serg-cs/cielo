/**
 * Small DOM helpers shared by every view.
 */

const SVG_NAMESPACE = "http://www.w3.org/2000/svg";

/**
 * Create an element.
 *
 * @param {string} tag Tag name, optionally with classes: `div.card.wide`.
 * @param {Record<string, unknown> | null} [attributes] Attributes; `on*`
 *   functions become listeners, `false`/`null`/`undefined` are skipped.
 * @param {...(Node | string | number | null | undefined | false | Array<unknown>)} children
 * @returns {HTMLElement}
 */
export function h(tag, attributes, ...children) {
  const [name, ...classes] = tag.split(".");
  const element = document.createElement(name);
  if (classes.length > 0) {
    element.className = classes.join(" ");
  }
  setAttributes(element, attributes);
  append(element, children);
  return element;
}

/**
 * Create an SVG element.
 *
 * @param {string} tag
 * @param {Record<string, unknown> | null} [attributes]
 * @param {...Node} children
 * @returns {SVGElement}
 */
export function svg(tag, attributes, ...children) {
  const element = document.createElementNS(SVG_NAMESPACE, tag);
  setAttributes(element, attributes);
  append(element, children);
  return element;
}

/**
 * An icon from the inline sprite.
 *
 * @param {string} name Icon name, such as `sun` or `chevron-left`.
 * @param {string} [className]
 * @returns {SVGElement}
 */
export function icon(name, className = "icon") {
  const element = svg(
    "svg",
    { class: className, "aria-hidden": "true", focusable: "false" },
    svg("use", { href: `#cielo-icon-${name}` }),
  );
  return element;
}

/**
 * A solid arrow showing where the wind blows to, from the direction it
 * comes from (in degrees).
 *
 * @param {number} degrees
 * @param {string} [className]
 */
export function windArrow(degrees, className = "icon icon-small") {
  return h(
    "span.wind-arrow",
    { style: { "--direction": `${degrees + 180}deg` }, "aria-hidden": "true" },
    svg(
      "svg",
      { class: className, viewBox: "0 0 24 24", focusable: "false" },
      svg("path", {
        d: "M12 2 19 21 12 17 5 21Z",
        fill: "currentColor",
        stroke: "currentColor",
        "stroke-width": "2",
        "stroke-linejoin": "round",
      }),
    ),
  );
}

/**
 * @param {Element} element
 * @param {Record<string, unknown> | null | undefined} attributes
 */
function setAttributes(element, attributes) {
  if (!attributes) {
    return;
  }
  for (const [key, value] of Object.entries(attributes)) {
    if (value === false || value === null || value === undefined) {
      continue;
    }
    if (key.startsWith("on") && typeof value === "function") {
      element.addEventListener(key.slice(2).toLowerCase(), /** @type {EventListener} */ (value));
    } else if (key === "style" && typeof value === "object") {
      for (const [property, propertyValue] of Object.entries(value)) {
        /** @type {HTMLElement} */ (element).style.setProperty(property, String(propertyValue));
      }
    } else if (key === "dataset" && typeof value === "object") {
      Object.assign(/** @type {HTMLElement} */ (element).dataset, value);
    } else {
      element.setAttribute(key, value === true ? "" : String(value));
    }
  }
}

/**
 * @param {Element | DocumentFragment} element
 * @param {Array<unknown>} children
 */
function append(element, children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) {
      continue;
    }
    element.append(child instanceof Node ? child : String(child));
  }
}

/**
 * Replace an element's children.
 *
 * @param {Element} element
 * @param {...unknown} children
 */
export function replaceChildren(element, ...children) {
  // Build first, then swap in one step, so the page never collapses and
  // loses its scroll position in between.
  const fragment = document.createDocumentFragment();
  append(fragment, children);
  element.replaceChildren(fragment);
}

/**
 * Re-render a container without losing keyboard focus: the focused element
 * is found again after rendering by its link, ID, action or form value.
 *
 * @param {Element} container
 * @param {() => void} render
 */
export function preserveFocus(container, render) {
  const active = document.activeElement;
  const key = active && container.contains(active) ? focusKey(active) : null;
  render();
  if (!key) return;
  const replacement = container.querySelector(key);
  if (replacement instanceof HTMLElement || replacement instanceof SVGElement) {
    replacement.focus({ preventScroll: true });
  }
}

/** @param {Element} element */
function focusKey(element) {
  const escape = (/** @type {string} */ value) => CSS.escape(value);
  if (element.id) return `#${escape(element.id)}`;
  const href = element.getAttribute("href");
  if (href) return `${element.tagName.toLowerCase()}[href="${escape(href)}"]`;
  const { id, action } = /** @type {HTMLElement} */ (element).dataset ?? {};
  if (id && action) return `[data-id="${escape(id)}"][data-action="${escape(action)}"]`;
  const name = element.getAttribute("name");
  const value = element.getAttribute("value");
  if (name && value) return `[name="${escape(name)}"][value="${escape(value)}"]`;
  if (element.classList.length) return `.${[...element.classList].map(escape).join(".")}`;
  return null;
}

/**
 * A small ⓘ button that opens the guide at a section (see `guide.js`).
 *
 * @param {string} section
 * @param {string} topic What it explains, for screen readers.
 */
export function infoButton(section, topic) {
  return h(
    "button.info-button",
    {
      type: "button",
      "data-guide": section,
      "aria-label": `Qué significa: ${topic}`,
    },
    icon("info", "icon icon-small"),
  );
}

/** @type {WeakMap<object, number>} */
const identities = new WeakMap();
let lastIdentity = 0;

/**
 * A number that stays the same for the same object, to tell whether a
 * view's data changed without comparing it field by field.
 *
 * @param {unknown} value
 */
export function identity(value) {
  if (typeof value !== "object" || value === null) return 0;
  let id = identities.get(value);
  if (id === undefined) {
    lastIdentity += 1;
    id = lastIdentity;
    identities.set(value, id);
  }
  return id;
}

/**
 * Let a bottom sheet be pulled down by its header to close it, as its
 * handle suggests. A short pull springs back.
 *
 * @param {HTMLDialogElement} dialog
 */
export function swipeToClose(dialog) {
  /** @type {{id: number, y: number, at: number} | null} */
  let drag = null;
  let offset = 0;
  const reset = () => {
    dialog.style.removeProperty("transform");
    dialog.style.removeProperty("transition");
  };
  dialog.addEventListener("pointerdown", (event) => {
    const header = event.target instanceof Element ? event.target.closest(".sheet-header") : null;
    // Buttons in the header keep their taps; only a phone-sized sheet moves.
    const onButton = event.target instanceof Element && event.target.closest("button");
    if (!header || onButton || matchMedia("(min-width: 48rem)").matches) return;
    drag = { id: event.pointerId, y: event.clientY, at: performance.now() };
    offset = 0;
    dialog.style.transition = "none";
    header.setPointerCapture(event.pointerId);
  });
  dialog.addEventListener("pointermove", (event) => {
    if (!drag || event.pointerId !== drag.id) return;
    offset = Math.max(0, event.clientY - drag.y);
    dialog.style.transform = `translateY(${offset}px)`;
  });
  const end = (/** @type {PointerEvent} */ event) => {
    if (!drag || event.pointerId !== drag.id) return;
    const speed = offset / Math.max(performance.now() - drag.at, 1);
    drag = null;
    if (offset > Math.min(140, dialog.offsetHeight / 4) || (offset > 40 && speed > 0.6)) {
      dialog.style.transition = "transform 0.18s ease-in";
      dialog.style.transform = "translateY(100%)";
      setTimeout(() => {
        dialog.close();
        reset();
      }, 170);
    } else {
      dialog.style.transition = "transform 0.2s ease-out";
      dialog.style.transform = "";
      setTimeout(reset, 220);
    }
  };
  dialog.addEventListener("pointerup", end);
  dialog.addEventListener("pointercancel", end);
  dialog.addEventListener("close", reset);
}
