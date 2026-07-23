const iconNamePattern = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;
const iconGlyphs = new Map([
/* @cielo-icon-glyphs */
]);

export class CieloIcon extends HTMLElement {
  static observedAttributes = ["name"];

  constructor() {
    super();
    this.attachShadow({ mode: "open" });
    this.#render();
  }

  connectedCallback() {
    this.setAttribute("aria-hidden", "true");
    this.#updateIcon();
  }

  attributeChangedCallback() {
    this.#updateIcon();
  }

  #updateIcon() {
    const slot = this.shadowRoot?.querySelector(".icon-slot");
    if (!(slot instanceof HTMLElement)) {
      return;
    }

    const name = this.getAttribute("name") ?? "";
    const glyph = iconNamePattern.test(name) ? iconGlyphs.get(name) : undefined;
    if (glyph === undefined) {
      slot.replaceChildren();
      return;
    }

    // Parse only the selected build-generated glyph for this component instance.
    slot.innerHTML = glyph;
    const icon = slot.querySelector("svg");
    if (!(icon instanceof SVGSVGElement)) {
      slot.replaceChildren();
      return;
    }

    icon.classList.add("icon");
    icon.setAttribute("aria-hidden", "true");
    icon.setAttribute("focusable", "false");
  }

  #render() {
    if (this.shadowRoot === null) {
      return;
    }

    this.shadowRoot.innerHTML = `
      <style>
        :host {
          display: inline-block;
          flex: 0 0 auto;
          width: 1em;
          height: 1em;
          color: inherit;
          line-height: 1;
          pointer-events: none;
          vertical-align: -0.125em;
        }

        .icon-slot,
        .icon {
          display: block;
          width: 100%;
          height: 100%;
        }

        .icon {
          fill: none;
          stroke-linecap: round;
          stroke-linejoin: round;
          stroke-width: 2;
        }
      </style>
      <span class="icon-slot"></span>
    `;
  }
}

customElements.define("cielo-icon", CieloIcon);
