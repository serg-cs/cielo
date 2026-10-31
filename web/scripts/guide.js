/**
 * The guide: one panel that explains every measurement in the app. It opens
 * from the info button on the list, or from a small ⓘ next to a value,
 * scrolled to that value's section. It is not a route: it has no links, and
 * a history entry of its own lets the back button close it.
 */

import { temperatureGradient } from "./cards.js";
import { h, icon } from "./dom.js";

/**
 * @typedef {object} GuideSection
 * @property {string} id
 * @property {string} icon
 * @property {string} title
 * @property {() => unknown} body
 */

/**
 * A short scale: a coloured mark, a name and what it means.
 *
 * @param {string} className Colour class of each mark, followed by its level.
 * @param {Array<[string, string]>} rows Name and meaning.
 */
const scale = (className, rows) =>
  h(
    "ul.guide-scale",
    { role: "list" },
    rows.map(([name, meaning], index) =>
      h(
        "li",
        null,
        h("span.guide-mark", {
          class: `guide-mark ${className}-${index + 1}`,
          "aria-hidden": "true",
        }),
        h("strong", null, name),
        h("span", null, meaning),
      ),
    ),
  );

/** @type {GuideSection[]} */
export const GUIDE_SECTIONS = [
  {
    id: "temperatura",
    icon: "thermometer",
    title: "Temperatura",
    body: () => [
      h("p", null, "La temperatura del aire a la sombra. Mín y máx son la más baja y la más alta del día."),
      h(
        "p",
        null,
        h("strong", null, "Sensación térmica: "),
        "cómo se nota en el cuerpo. Con viento se siente más frío; con calor y humedad, más calor.",
      ),
      h(
        "div.guide-gradient",
        null,
        h("span.guide-gradient-bar", {
          style: { background: temperatureGradient(-10, 40) },
          "aria-hidden": "true",
        }),
        h(
          "span.guide-gradient-labels",
          null,
          ["-10°", "0°", "10°", "20°", "30°", "40°"].map((label) => h("span", null, label)),
        ),
      ),
    ],
  },
  {
    id: "precipitacion",
    icon: "umbrella",
    title: "Lluvia y nieve",
    body: () => [
      h(
        "p",
        null,
        h("strong", null, "Probabilidad (%): "),
        "posibilidad de que llueva algo en ese periodo. No dice cuánto ni cuánto rato: un 60% puede ser un chubasco corto.",
      ),
      h(
        "p",
        null,
        h("strong", null, "Cantidad (mm): "),
        "litros por metro cuadrado. Si solo caen unas gotas, menos de 0,1\u00a0mm, se indica «unas gotas».",
      ),
      scale("guide-rain", [
        ["Hasta 2 mm en una hora", "débil"],
        ["2 a 15 mm", "moderada"],
        ["15 a 30 mm", "fuerte"],
        ["Más de 30 mm", "muy fuerte"],
      ]),
      h("p.guide-note", null, "La tormenta y la nieve también se dan como probabilidad, cuando AEMET la prevé."),
    ],
  },
  {
    id: "viento",
    icon: "wind",
    title: "Viento",
    body: () => [
      h(
        "p",
        null,
        h("strong", null, "Viento medio: "),
        "su velocidad habitual. ",
        h("strong", null, "Rachas: "),
        "los golpes más fuertes, de pocos segundos.",
      ),
      h(
        "p",
        null,
        h("strong", null, "Dirección: "),
        "la flecha apunta hacia donde sopla; el nombre dice de dónde viene. Viento «del norte» sopla hacia el sur.",
      ),
      scale("guide-wind", [
        ["Calma", "menos de 6 km/h"],
        ["Flojo", "6 a 20 km/h"],
        ["Moderado", "21 a 40 km/h"],
        ["Fuerte", "41 a 70 km/h"],
        ["Muy fuerte", "71 a 120 km/h"],
      ]),
    ],
  },
  {
    id: "humedad",
    icon: "droplet",
    title: "Humedad",
    body: () => [
      h("p", null, "Cuánto vapor de agua tiene el aire, del 0% al 100%."),
      h(
        "p",
        null,
        "Por encima del 80% el ambiente se nota húmedo: bochorno si hace calor, niebla o rocío si hace fresco. Por debajo del 30%, aire seco.",
      ),
    ],
  },
  {
    id: "uv",
    icon: "sun-dim",
    title: "Índice UV",
    body: () => [
      h("p", null, "La fuerza de la radiación del sol a mediodía, que quema la piel."),
      scale("guide-uv", [
        ["0–2 Bajo", "no hace falta protección"],
        ["3–5 Moderado", "protección solar a mediodía"],
        ["6–7 Alto", "protección y sombra a mediodía"],
        ["8–10 Muy alto", "protección extra; evita el sol a mediodía"],
        ["11+ Extremo", "evita el sol en las horas centrales"],
      ]),
    ],
  },
  {
    id: "incendios",
    icon: "flame",
    title: "Riesgo de incendio",
    body: () => [
      h(
        "p",
        null,
        "Lo fácil que es que empiece y se extienda un incendio forestal, según el tiempo y lo seca que está la vegetación.",
      ),
      scale("guide-fire", [
        ["Muy bajo", "es difícil que prenda"],
        ["Bajo", "riesgo pequeño"],
        ["Moderado", "cuidado con el fuego"],
        ["Alto", "extrema la precaución en el campo"],
        ["Muy alto", "no hagas fuego"],
        ["Extremo", "cualquier chispa puede ser un incendio"],
      ]),
    ],
  },
  {
    id: "nieve",
    icon: "mountain-snow",
    title: "Cota de nieve",
    body: () => [
      h("p", null, "La altitud a partir de la que puede nevar si hay precipitación."),
      h("p", null, "Si el municipio está por encima de la cota, la lluvia puede caer como nieve."),
    ],
  },
  {
    id: "sol",
    icon: "sunrise",
    title: "Sol",
    body: () => [h("p", null, "Hora de amanecer y de atardecer en el lugar, y las horas de luz entre ambas.")],
  },
  {
    id: "avisos",
    icon: "triangle-alert",
    title: "Avisos",
    body: () => [
      h("p", null, "Avisos oficiales de AEMET por fenómenos peligrosos en la zona del lugar."),
      scale("guide-warning", [
        ["Amarillo", "riesgo para actividades concretas"],
        ["Naranja", "riesgo importante; mantente informado"],
        ["Rojo", "riesgo extraordinario; sigue las indicaciones de las autoridades"],
      ]),
      h("p.guide-note", null, "El umbral es el valor que se espera superar, como la lluvia en una hora."),
    ],
  },
  {
    id: "playas",
    icon: "parasol",
    title: "Playas",
    body: () => [
      h("p", null, "Previsión de mañana y tarde para tres días."),
      scale("guide-level", [
        ["Flojo o débil", "viento y oleaje tranquilos"],
        ["Moderado", "precaución en el agua"],
        ["Fuerte", "baño peligroso"],
      ]),
      h("p", null, h("strong", null, "Agua: "), "temperatura del mar en la playa."),
    ],
  },
  {
    id: "datos",
    icon: "clock",
    title: "Los datos",
    body: () => [
      h(
        "p",
        null,
        "Previsión de AEMET, la Agencia Estatal de Meteorología. Se revisa cada hora; AEMET la renueva varias veces al día.",
      ),
      h("p", null, "Las horas son las del lugar. Si una previsión tiene más de 6 horas, se indica."),
    ],
  },
];

export class Guide {
  /** @param {HTMLDialogElement} dialog */
  constructor(dialog) {
    this.dialog = dialog;
    this.body = h(
      "div.sheet-body.guide-body",
      null,
      h("nav.guide-index", { "aria-label": "Secciones" }, [
        h(
          "ul",
          { role: "list" },
          GUIDE_SECTIONS.map((section) =>
            h(
              "li",
              null,
              h(
                "button.guide-chip",
                { type: "button", onclick: () => this.scrollTo(section.id) },
                icon(section.icon, "icon icon-small"),
                section.title,
              ),
            ),
          ),
        ),
      ]),
      GUIDE_SECTIONS.map((section) =>
        h(
          "section.guide-section",
          {
            id: `guide-${section.id}`,
            "aria-labelledby": `guide-${section.id}-title`,
          },
          h("h3", { id: `guide-${section.id}-title` }, icon(section.icon, "icon icon-small"), section.title),
          section.body(),
        ),
      ),
    );
    dialog.append(
      h(
        "header.sheet-header",
        null,
        h("span.sheet-handle", { "aria-hidden": "true" }),
        h("h2.sheet-title", { id: "guide-title", tabindex: "-1" }, "Cómo leer la previsión"),
        h(
          "button.icon-button",
          {
            type: "button",
            "aria-label": "Cerrar",
            onclick: () => this.dialog.close(),
          },
          icon("x"),
        ),
      ),
      this.body,
    );
    dialog.addEventListener("click", (event) => {
      if (event.target === dialog) dialog.close();
    });
    // Closing by the button, Escape or the backdrop drops the guide's
    // history entry; the back button closes it through `popstate`.
    dialog.addEventListener("close", () => {
      if (/** @type {{guide?: boolean} | null} */ (history.state)?.guide) history.back();
    });
    window.addEventListener("popstate", () => {
      if (dialog.open && !(/** @type {{guide?: boolean} | null} */ (history.state)?.guide)) dialog.close();
    });
  }

  /**
   * Open the guide, at a section if given.
   *
   * @param {string} [section]
   */
  open(section) {
    if (!this.dialog.open) {
      const pageScroll = window.scrollY;
      history.pushState({ ...history.state, guide: true }, "", location.hash);
      this.dialog.showModal();
      if (window.scrollY !== pageScroll) window.scrollTo(0, pageScroll);
    }
    // After the dialog has its layout, so the position is not lost.
    const place = () => (section ? this.scrollTo(section) : this.body.scrollTo({ top: 0, behavior: "instant" }));
    place();
    requestAnimationFrame(place);
    /** @type {HTMLElement | null} */ (this.dialog.querySelector(".sheet-title"))?.focus({ preventScroll: true });
  }

  /** @param {string} section */
  scrollTo(section) {
    const target = this.body.querySelector(`#guide-${section}`);
    if (!(target instanceof HTMLElement)) return;
    this.body.scrollTo({
      top: target.offsetTop - this.body.offsetTop - 8,
      behavior: "instant",
    });
    target.classList.remove("highlight");
    void target.offsetWidth;
    target.classList.add("highlight");
  }
}
