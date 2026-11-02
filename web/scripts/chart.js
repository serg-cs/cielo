/**
 * Small hour-by-hour charts drawn as SVG, with HTML text so they follow the
 * text size. Hovering, dragging a finger or using the arrow keys shows a
 * pill above the chart with the values of the selected hour. No library:
 * a few kilobytes instead of tens.
 */

import { h, icon, infoButton, svg, windArrow } from "./dom.js";

let chartCount = 0;

/**
 * @typedef {object} Series
 * @property {Array<number | null>} values One value per point.
 * @property {"line" | "bars" | "step"} kind `step` holds each value for its hour.
 * @property {string} className Colour and stroke style.
 * @property {[number, number]} [range] Own scale (bottom, top), such as 0-100 %.
 * @property {Array<[number, string]>} [colors] Colour stops by value (value,
 *   CSS colour): the line takes the colour of the value it passes through.
 * @property {boolean} [hard] Colour stops are hard edges (levels), not a blend.
 * @property {boolean} [area] Fill below the line or step.
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
 * @property {string} [unit] Unit of the scale, shown after the title.
 * @property {string} iconName
 * @property {string} summary Spoken description of the whole chart.
 * @property {number[]} hours Local hour (0-23) of each point.
 * @property {Series[]} series
 * @property {[number, number]} range Value at the bottom and top.
 * @property {(index: number) => PillContent} describe Pill content for a point.
 * @property {(value: number) => string} label Axis label for a value.
 * @property {Array<[string, string]>} [key] Series class and name.
 * @property {string} [guide] Guide section the title's ⓘ opens.
 * @property {number | null} [now] Current hour (0-24, fractional) on today's charts.
 * @property {Array<{index: number, degrees: number | null, level: number | null}>} [arrows]
 *   Wind directions shown under the plot.
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
 * Stops of an SVG gradient that colours by value, on a vertical axis.
 *
 * @param {Array<[number, string]>} colors Value and colour, ascending.
 * @param {(value: number) => number} y Value to drawing height.
 * @param {string} id
 * @param {boolean} hard Hard edges between colours (levels).
 */
function valueGradient(colors, y, id, hard) {
  const top = colors[colors.length - 1][0];
  const bottom = colors[0][0];
  const offset = (/** @type {number} */ value) => ((top - value) / (top - bottom || 1)).toFixed(4);
  // From the top (highest value) down. With hard edges each colour covers
  // from its value up to the next one.
  const stops = [svg("stop", { offset: 0, style: { "stop-color": colors[colors.length - 1][1] } })];
  for (let index = colors.length - 1; index >= 1; index -= 1) {
    const [value, color] = colors[index];
    if (hard) {
      stops.push(svg("stop", { offset: offset(value), style: { "stop-color": color } }));
      stops.push(svg("stop", { offset: offset(value), style: { "stop-color": colors[index - 1][1] } }));
    } else {
      stops.push(svg("stop", { offset: offset(value), style: { "stop-color": color } }));
    }
  }
  stops.push(svg("stop", { offset: 1, style: { "stop-color": colors[0][1] } }));
  return svg(
    "linearGradient",
    { id, gradientUnits: "userSpaceOnUse", x1: 0, x2: 0, y1: y(top), y2: y(bottom) },
    ...stops,
  );
}

/**
 * One chart: a plot with its scale on the right, the hours below and a
 * pill with the values of the hour under the finger.
 *
 * @param {ChartOptions} options
 */
export function chart({
  title,
  unit,
  iconName,
  summary,
  hours,
  series,
  range,
  describe,
  label,
  key = [],
  guide,
  now = null,
  arrows = [],
}) {
  chartCount += 1;
  const uid = `chart-${chartCount}`;
  const percentX = (/** @type {number} */ index) => ((hours[index] + 0.5) / 24) * 100;
  const scale =
    (/** @type {[number, number]} */ [low, high]) =>
    (/** @type {number} */ value) =>
      100 - ((value - low) / (high - low || 1)) * 100;

  const defs = svg("defs");
  const plot = svg("svg", {
    class: "chart-plot",
    viewBox: "0 0 24 100",
    preserveAspectRatio: "none",
    "aria-hidden": "true",
  });
  plot.append(defs);
  for (const y of [0, 50, 100]) {
    plot.append(svg("line", { class: "chart-grid", x1: 0, x2: 24, y1: y, y2: y }));
  }
  for (const x of [6, 12, 18]) {
    plot.append(svg("line", { class: "chart-grid vertical", x1: x, x2: x, y1: 0, y2: 100 }));
  }

  /** Drawn height (%) of each line point, for the dots shown with the pill. @type {Array<{className: string, y: Array<number | null>}>} */
  const lines = [];
  series.forEach((item, seriesIndex) => {
    const y = scale(item.range ?? range);
    let stroke = null;
    if (item.colors) {
      const id = `${uid}-colors-${seriesIndex}`;
      defs.append(valueGradient(item.colors, y, id, item.hard ?? false));
      stroke = `url(#${id})`;
    }
    const fadeId = `${uid}-fade-${seriesIndex}`;
    if (item.area) {
      defs.append(
        svg(
          "linearGradient",
          { id: fadeId, class: item.className, x1: 0, x2: 0, y1: 0, y2: 1 },
          svg("stop", { offset: 0, class: "fade-start" }),
          svg("stop", { offset: 1, class: "fade-end" }),
        ),
      );
    }
    if (item.kind === "bars") {
      item.values.forEach((value, index) => {
        if (!value) return;
        const top = Math.min(y(value), 97);
        plot.append(
          svg("rect", {
            class: `chart-bar ${item.className}`,
            x: hours[index] + 0.14,
            width: 0.72,
            y: top,
            height: 100 - top,
          }),
        );
      });
      return;
    }
    if (item.kind === "step") {
      let path = "";
      let area = "";
      item.values.forEach((value, index) => {
        if (value === null) return;
        const joined = index > 0 && item.values[index - 1] !== null && hours[index - 1] === hours[index] - 1;
        path += `${joined ? "V" : `M${hours[index]},`}${y(value)} H${hours[index] + 1} `;
        area += `M${hours[index]},100 V${y(value)} H${hours[index] + 1} V100 Z `;
      });
      if (item.area && area) {
        plot.append(svg("path", { class: `chart-area-fill ${item.className}`, d: area, fill: `url(#${fadeId})` }));
      }
      if (path) plot.append(svg("path", { class: `chart-line ${item.className}`, d: path.trim() }));
      return;
    }
    // Softening would move a value across a level edge and give the line
    // another colour than the hour's own, so lines coloured by level keep
    // their real values.
    const drawn = (item.hard ? item.values : softened(item.values)).map((value) =>
      value === null ? null : y(value),
    );
    const points = drawn.map((value, index) =>
      value === null ? null : /** @type {[number, number]} */ ([hours[index] + 0.5, value]),
    );
    const line = smoothPath(points);
    // The area under each unbroken run of points (gaps leave no area).
    /** @type {Array<Array<[number, number]>>} */
    const runs = [[]];
    for (const point of points) {
      if (point) runs[runs.length - 1].push(point);
      else if (runs[runs.length - 1].length) runs.push([]);
    }
    const area = runs
      .filter((run) => run.length > 1)
      .map((run) => `${smoothPath(run)} L${run[run.length - 1][0]},100 L${run[0][0]},100 Z`)
      .join(" ");
    if (item.area && area) {
      // A coloured area fades out downwards through a mask.
      const maskId = `${uid}-mask-${seriesIndex}`;
      if (stroke) {
        defs.append(
          svg(
            "linearGradient",
            { id: `${maskId}-fade`, x1: 0, x2: 0, y1: 0, y2: 1 },
            svg("stop", { offset: 0, "stop-color": "#fff", "stop-opacity": "0.5" }),
            svg("stop", { offset: 1, "stop-color": "#fff", "stop-opacity": "0" }),
          ),
          svg(
            "mask",
            { id: maskId, maskUnits: "userSpaceOnUse", x: 0, y: 0, width: 24, height: 100 },
            svg("rect", { x: 0, y: 0, width: 24, height: 100, fill: `url(#${maskId}-fade)` }),
          ),
        );
      }
      plot.append(
        svg("path", {
          class: `chart-area-fill ${item.className}`,
          d: area,
          fill: stroke ?? `url(#${fadeId})`,
          mask: stroke ? `url(#${maskId})` : null,
        }),
      );
    }
    plot.append(
      svg("path", {
        class: `chart-line ${item.className}`,
        d: line,
        style: stroke ? { stroke } : null,
      }),
    );
    lines.push({ className: item.className, y: drawn });
  });

  const cursor = h("span.chart-cursor", { hidden: true });
  const dots = lines.map((line) => h("span.chart-dot", { class: `chart-dot ${line.className}`, hidden: true }));
  const pill = h("div.chart-pill", { hidden: true, "aria-live": "polite" });
  /** @type {number | null} */
  let selected = null;

  const nowLine =
    now === null
      ? null
      : h(
          "span.chart-now",
          { style: { "--x": `${(now / 24) * 100}%` }, "aria-hidden": "true" },
          h("span.chart-now-label", null, "Ahora"),
        );

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
    area.classList.toggle("selecting", index !== null);
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
    nowLine,
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
      unit ? h("small.chart-unit", null, unit) : null,
      guide ? infoButton(guide, title.toLowerCase()) : null,
    ),
    area,
    arrows.length
      ? h(
          "div.chart-arrows",
          { "aria-hidden": "true" },
          arrows.map((arrow, arrowIndex) =>
            h(
              "span",
              {
                // Every other one goes when the chart is narrow.
                class: `wind-level-${arrow.level ?? 0}${arrowIndex % 2 ? " minor" : ""}`,
                style: { "--x": `${percentX(arrow.index)}%` },
              },
              arrow.degrees === null ? h("span.chart-calm") : windArrow(arrow.degrees, "icon icon-small"),
            ),
          ),
        )
      : null,
    h(
      "div.chart-hours",
      { "aria-hidden": "true" },
      [0, 6, 12, 18, 24].map((hour) =>
        h("span", { style: { "--x": `${(hour / 24) * 100}%` } }, String(hour)),
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
