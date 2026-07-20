const iconUrls = new Map(
  [
    "circle-x",
    "list",
    "map-pin",
    "search",
    "trash-2",
  ].map((name) => [
    name,
    new URL(`../icons/${name}.svg`, import.meta.url),
  ]),
);

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
    const glyph = this.shadowRoot?.querySelector(".glyph");
    if (!(glyph instanceof HTMLElement)) {
      return;
    }

    const name = this.getAttribute("name") ?? "";
    const iconUrl = iconUrls.get(name);
    if (iconUrl === undefined) {
      glyph.hidden = true;
      return;
    }

    // Use the SVG alpha channel while inheriting color from the caller.
    const maskImage = `url("${iconUrl.href}")`;
    glyph.hidden = false;
    glyph.style.maskImage = maskImage;
    glyph.style.webkitMaskImage = maskImage;
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

        .glyph {
          display: block;
          width: 100%;
          height: 100%;
          background: currentcolor;
          mask-position: center;
          mask-repeat: no-repeat;
          mask-size: contain;
          -webkit-mask-position: center;
          -webkit-mask-repeat: no-repeat;
          -webkit-mask-size: contain;
        }

        .glyph[hidden] {
          display: none;
        }
      </style>
      <span class="glyph"></span>
    `;
  }
}

customElements.define("cielo-icon", CieloIcon);
