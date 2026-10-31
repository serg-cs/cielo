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
