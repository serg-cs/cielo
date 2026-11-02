/**
 * Header of a place screen: back (to the previous screen), name, a badge when the data is old, and
 * a save button that only shows while the place is not saved.
 */

import { h, icon, replaceChildren } from "./dom.js";

export class PlaceHeader {
  /**
   * @param {object} callbacks
   * @param {() => void} callbacks.onBack
   * @param {() => void} callbacks.onSave
   * @param {string} titleId
   */
  constructor({ onBack, onSave }, titleId) {
    this.title = h("h1.place-title", { id: titleId, tabindex: "-1" });
    this.subtitle = h("p.place-subtitle");
    this.saveButton = h(
      "button.save-button",
      { type: "button", onclick: onSave },
      icon("plus", "icon icon-small"),
      h("span", null, "Guardar"),
    );
    this.toast = h("p.toast", { role: "status" });
    this.stale = h("p.stale-badge", { role: "status", hidden: true });
    this.element = h(
      "header.place-header",
      null,
      h(
        "button.icon-button.back-button",
        {
          type: "button",
          "aria-label": "Volver",
          onclick: onBack,
        },
        icon("chevron-left"),
      ),
      h("div.place-heading", null, this.title, this.subtitle),
      this.stale,
      this.saveButton,
      this.toast,
    );
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    this.toastTimer = undefined;
  }

  /**
   * @param {{title: string, subtitle: string, saved: boolean, known: boolean, stale?: string | null}} state
   *   `known` is false while the place is unknown, which hides the save button.
   *   `stale` is the note for an old forecast, shown as a badge.
   */
  update({ title, subtitle, saved, known, stale = null }) {
    this.stale.hidden = !stale;
    if (stale && this.stale.title !== stale) {
      this.stale.title = stale;
      replaceChildren(
        this.stale,
        icon("clock", "icon icon-tiny"),
        h("span.stale-long", { "aria-hidden": "true" }, "Hace más de 6\u00a0h"),
        h("span.stale-short", { "aria-hidden": "true" }, "+6\u00a0h"),
        h("span.visually-hidden", null, stale),
      );
    }
    this.title.textContent = title;
    this.title.title = title;
    this.subtitle.textContent = subtitle;
    this.subtitle.hidden = !subtitle;
    // A name alone (no province) gets a shorter bar, so it does not look empty.
    this.element.classList.toggle("title-only", !subtitle);
    this.saveButton.hidden = saved || !known;
    this.saveButton.setAttribute("aria-label", `Guardar ${title} en mis lugares`);
  }

  /** Confirm a save briefly, where the button was. */
  confirmSaved() {
    replaceChildren(this.toast, icon("check", "icon icon-small"), "Guardado");
    this.toast.classList.add("visible");
    clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => {
      this.toast.classList.remove("visible");
      this.toast.textContent = "";
    }, 2500);
  }

  focusTitle() {
    this.title.focus({ preventScroll: true });
  }
}
