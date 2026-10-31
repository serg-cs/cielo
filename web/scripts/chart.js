/**
 * Small hour-by-hour charts drawn as SVG, with HTML text so they follow the
 * text size. Hovering, dragging a finger or using the arrow keys shows a
 * pill above the chart with the values of the selected hour. No library:
 * a few kilobytes instead of tens.
 */

import { h, icon, infoButton, svg, windArrow } from "./dom.js";

/**
 * @typedef {object} Series
 * @property {Array<number | null>} values One value per point.
 * @property {"line" | "bars" | "step"} kind `step` holds each value for its hour.
 * @property {string} className Colour and stroke style.
 * @property {[number, number]} [range] Own scale (bottom, top), such as 0-100 %.
 */

/**
 * One value in the pill, marked like its series so it is clear which line
 * or bar it reads.
 *
 * @typedef {object} PillReading
 * @property {string} value Such as `23°` or `0,4 mm`.
 * @property {string} [label] Such as `rachas`.
 * @property {string} [className] Series class, for the colour mark.
 * @property {number | null} [arrow] Wind direction in degrees, shown instead of the mark.
 */

/**
 * @typedef {object} PillContent
 * @property {string} time
 * @property {PillReading[]} readings
 */

/**
 * @typedef {object} ChartOptions
 * @property {string} title
 * @property {string} iconName
 * @property {string} summary Spoken description of the whole chart.
 * @property {number[]} hours Local hour (0-23) of each point.
 * @property {Series[]} series
 * @property {[number, number]} range Value at the bottom and top.
 * @property {(index: number) => PillContent} describe Pill content for a point.
 * @property {(value: number) => string} label Axis label for a value.
 * @property {Array<[string, string]>} [key] Series class and name.
 * @property {string} [guide] Guide section the title's ⓘ opens.
 */

/**
 * Smooth SVG path through points (Catmull-Rom curves). Control points are
 * kept between their segment's two values, so the line never overshoots
 * the data. Gaps split the path.
 *
 * @param {Array<[number, number] | null>} points
 */
export function smoothPath(points) {
  /** @type {Array<Array<[number, number]>>} */
  const runs = [[]];
  for (const point of points) {
    if (point) runs[runs.length - 1].push(point);
    else if (runs[runs.length - 1].length) runs.push([]);
  }
  const clamp = (/** @type {number} */ value, /** @type {number} */ a, /** @type {number} */ b) =>
    Math.min(Math.max(value, Math.min(a, b)), Math.max(a, b));
  return runs
    .filter((run) => run.length)
    .map((run) => {
      let path = `M${run[0][0]},${run[0][1]}`;
      for (let index = 1; index < run.length; index += 1) {
        const [x0, y0] = run[Math.max(index - 2, 0)];
        const [x1, y1] = run[index - 1];
        const [x2, y2] = run[index];
        const [x3, y3] = run[Math.min(index + 1, run.length - 1)];
        const c1 = [x1 + (x2 - x0) / 6, clamp(y1 + (y2 - y0) / 6, y1, y2)];
        const c2 = [x2 - (x3 - x1) / 6, clamp(y2 - (y3 - y1) / 6, y1, y2)];
        path += ` C${c1[0]},${c1[1]} ${c2[0]},${c2[1]} ${x2},${y2}`;
      }
      return path;
    })
    .join(" ");
}

/**
 * Light smoothing for drawing whole-number data such as temperatures, so a
 * line does not climb in steps. Each value is averaged with its neighbours
 * (1-2-1); the first and last values and gaps are kept.
 *
 * @param {Array<number | null>} values
 * @returns {Array<number | null>}
 */
export function softened(values) {
  return values.map((value, index) => {
    const before = values[index - 1];
    const after = values[index + 1];
    if (value === null || before == null || after == null) return value;
    return (before + 2 * value + after) / 4;
  });
}

/**
 * A rounded range for values, at least `minimumSpan` wide.
 *
 * @param {Array<number | null>} values
 * @param {{minimumSpan?: number, floor?: number, ceiling?: number, step?: number}} [options]
 * @returns {[number, number]}
 */
export function chartRange(values, { minimumSpan = 4, floor, ceiling, step = 1 } = {}) {
  const present = /** @type {number[]} */ (values.filter((value) => value !== null && Number.isFinite(value)));
  let low = floor ?? (present.length ? Math.min(...present) : 0);
  let high = ceiling ?? (present.length ? Math.max(...present) : minimumSpan);
  if (high - low < minimumSpan) {
    if (floor !== undefined) high = low + minimumSpan;
    else if (ceiling !== undefined) low = high - minimumSpan;
    else {
      const middle = (low + high) / 2;
      low = middle - minimumSpan / 2;
      high = middle + minimumSpan / 2;
    }
  }
  return [Math.floor(low / step) * step, Math.ceil(high / step) * step];
}

/**
 * One chart.
 *
 * @param {ChartOptions} options
 */
export function chart({ title, iconName, summary, hours, series, range, describe, label, key = [], guide }) {
  const percentX = (/** @type {number} */ index) => ((hours[index] + 0.5) / 24) * 100;
  const scale =
    (/** @type {[number, number]} */ [low, high]) =>
    (/** @type {number} */ value) =>
      100 - ((value - low) / (high - low || 1)) * 100;

  const plot = svg("svg", {
    class: "chart-plot",
    viewBox: "0 0 24 100",
    preserveAspectRatio: "none",
    "aria-hidden": "true",
  });
  for (const y of [0, 50, 100]) {
    plot.append(svg("line", { class: "chart-grid", x1: 0, x2: 24, y1: y, y2: y }));
  }

  /** Drawn height (%) of each line point, for the dots shown with the pill. @type {Array<{className: string, y: Array<number | null>}>} */
  const lines = [];
  for (const item of series) {
    const y = scale(item.range ?? range);
    if (item.kind === "bars") {
      item.values.forEach((value, index) => {
        if (!value) return;
        const top = Math.min(y(value), 97);
        plot.append(
          svg("rect", {
            class: `chart-bar ${item.className}`,
            x: hours[index] + 0.18,
            width: 0.64,
            y: top,
            height: 100 - top,
          }),
        );
      });
    } else if (item.kind === "step") {
      let path = "";
      item.values.forEach((value, index) => {
        if (value === null) return;
        const joined = index > 0 && item.values[index - 1] !== null && hours[index - 1] === hours[index] - 1;
        path += `${joined ? "V" : `M${hours[index]},`}${y(value)} H${hours[index] + 1} `;
      });
      if (path)
        plot.append(
          svg("path", {
            class: `chart-line ${item.className}`,
            d: path.trim(),
          }),
        );
    } else {
      const drawn = softened(item.values).map((value) => (value === null ? null : y(value)));
      const points = drawn.map((value, index) =>
        value === null ? null : /** @type {[number, number]} */ ([hours[index] + 0.5, value]),
      );
      plot.append(
        svg("path", {
          class: `chart-line ${item.className}`,
          d: smoothPath(points),
        }),
      );
      lines.push({ className: item.className, y: drawn });
    }
  }

  const cursor = h("span.chart-cursor", { hidden: true });
  const dots = lines.map((line) => h("span.chart-dot", { class: `chart-dot ${line.className}`, hidden: true }));
  const pill = h("div.chart-pill", { hidden: true, "aria-live": "polite" });
  /** @type {number | null} */
  let selected = null;

  /** @param {PointerEvent} event */
  const pointAt = (event) => {
    const box = area.getBoundingClientRect();
    const hour = ((event.clientX - box.left) / box.width) * 24 - 0.5;
    let index = 0;
    hours.forEach((value, candidate) => {
      if (Math.abs(value - hour) < Math.abs(hours[index] - hour)) index = candidate;
    });
    return index;
  };

  /** @param {number | null} index */
  const select = (index) => {
    selected = index;
    cursor.hidden = index === null;
    pill.hidden = index === null;
    dots.forEach((dot, line) => {
      dot.hidden = index === null || lines[line].y[index] === null;
    });
    if (index === null) return;
    const { time, readings } = describe(index);
    pill.replaceChildren(
      h("span.chart-pill-time", null, time),
      h(
        "span.chart-pill-readings",
        null,
        readings.map((reading) =>
          h(
            "span.chart-reading",
            null,
            typeof reading.arrow === "number"
              ? windArrow(reading.arrow, "icon icon-tiny")
              : reading.className
                ? h("span", { class: `chart-swatch ${reading.className}` })
                : null,
            h("strong", null, reading.value),
            reading.label ? h("small", null, reading.label) : null,
          ),
        ),
      ),
    );

    cursor.style.setProperty("--x", `${percentX(index)}%`);
    dots.forEach((dot, line) => {
      dot.style.setProperty("--x", `${percentX(index)}%`);
      dot.style.setProperty("--y", `${lines[line].y[index] ?? 0}%`);
    });
    // Centre the pill on the hour, but keep it inside the chart.
    const width = area.clientWidth;
    const centre = (percentX(index) / 100) * width;
    const left = Math.min(Math.max(centre - pill.offsetWidth / 2, 0), Math.max(width - pill.offsetWidth, 0));
    pill.style.setProperty("--left", `${left}px`);
  };

  const area = h(
    "div.chart-area",
    {
      tabindex: "0",
      role: "img",
      "aria-label": `${title}: ${summary}`,
      onpointerdown: (/** @type {PointerEvent} */ event) => select(pointAt(event)),
      onpointermove: (/** @type {PointerEvent} */ event) => select(pointAt(event)),
      onpointerleave: () => select(null),
      onpointercancel: () => select(null),
      onpointerup: (/** @type {PointerEvent} */ event) => {
        if (event.pointerType !== "mouse") select(null);
      },
      onkeydown: (/** @type {KeyboardEvent} */ event) => {
        const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
        if (step) {
          event.preventDefault();
          select(Math.min(Math.max((selected ?? (step > 0 ? -1 : hours.length)) + step, 0), hours.length - 1));
        } else if (event.key === "Escape") {
          select(null);
        }
      },
      onblur: () => select(null),
    },
    plot,
    cursor,
    dots,
    pill,
    h("span.chart-label.top", { "aria-hidden": "true" }, label(range[1])),
    h("span.chart-label.middle", { "aria-hidden": "true" }, label((range[0] + range[1]) / 2)),
    h("span.chart-label.bottom", { "aria-hidden": "true" }, label(range[0])),
  );

  return h(
    "figure.chart",
    null,
    h(
      "figcaption.chart-title",
      null,
      icon(iconName, "icon icon-small"),
      title,
      guide ? infoButton(guide, title.toLowerCase()) : null,
    ),
    area,
    h(
      "div.chart-hours",
      { "aria-hidden": "true" },
      [0, 6, 12, 18, 24].map((hour) =>
        h("span", { style: { "--x": `${(hour / 24) * 100}%` } }, hour === 24 ? "24\u00a0h" : String(hour)),
      ),
    ),
    key.length
      ? h(
          "ul.chart-key",
          { role: "list", "aria-hidden": "true" },
          key.map(([className, name]) => h("li", null, h("span", { class: `chart-swatch ${className}` }), name)),
        )
      : null,
  );
}
