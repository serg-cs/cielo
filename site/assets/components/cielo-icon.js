const iconNamePattern = /^[a-z0-9]+(?:-[a-z0-9]+)*$/u;
const spriteUrl = new URL("../icons.svg", import.meta.url);

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
    const icon = this.shadowRoot?.querySelector(".icon");
    const use = this.shadowRoot?.querySelector("use");
    if (!(icon instanceof SVGSVGElement) || !(use instanceof SVGUseElement)) {
      return;
    }

    const name = this.getAttribute("name") ?? "";
    if (!iconNamePattern.test(name)) {
      icon.hidden = true;
      use.removeAttribute("href");
      return;
    }

    // Resolve every validated name against the one generated sprite resource.
    use.setAttribute("href", `${spriteUrl.href}#${name}`);
    icon.hidden = false;
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

        .icon {
          display: block;
          width: 100%;
          height: 100%;
          fill: none;
          stroke: currentcolor;
          stroke-linecap: round;
          stroke-linejoin: round;
          stroke-width: 2;
        }

        .icon[hidden] {
          display: none;
        }
      </style>
      <svg
        class="icon"
        viewBox="0 0 24 24"
        aria-hidden="true"
        focusable="false"
      >
        <use></use>
      </svg>
    `;
  }
}

customElements.define("cielo-icon", CieloIcon);
