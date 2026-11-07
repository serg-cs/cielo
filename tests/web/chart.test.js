import assert from "node:assert/strict";
import { test } from "node:test";

import { relativeLevels } from "../../web/scripts/card-hourly.js";
import { smoothPath, softened } from "../../web/scripts/chart.js";
import { hoursSummary } from "../../web/scripts/forecast.js";

/** Units are joined to numbers with a no-break space; compare as plain text. */
const plain = (/** @type {string} */ text) => text.replace(/\u00a0/g, " ");
import {
  addDays,
  builtAt,
  dayName,
  duration,
  localDate,
  millimetres,
  relativeHour,
} from "../../web/scripts/format.js";

test("temperature levels run from 0 to 1 and keep gaps", () => {
  const levels = relativeLevels([10, null, 20]);
  assert.deepEqual(levels, [0, null, 1]);
  assert.deepEqual(relativeLevels([null, null]), [null, null]);
});

test("flat temperatures sit in the middle", () => {
  assert.deepEqual(relativeLevels([5, 5, 5]), [0.5, 0.5, 0.5]);
});

test("local dates follow the municipality time zone", () => {
  const time = Date.parse("2026-10-31T23:30:00Z");
  assert.equal(localDate(time, "Europe/Madrid"), "2026-11-01");
  assert.equal(localDate(time, "Atlantic/Canary"), "2026-10-31");
});

test("Spanish day names and durations", () => {
  assert.equal(addDays("2026-12-31", 1), "2027-01-01");
  assert.equal(dayName("2026-11-01", "2026-11-01"), "Hoy");
  assert.equal(dayName("2026-11-02", "2026-11-01"), "Mañana");
  assert.equal(dayName("2026-11-03", "2026-11-01"), "Martes");
  assert.equal(plain(duration("07:42", "18:13")), "10 h 31 min");
  assert.equal(plain(millimetres("Ip")), "unas gotas");
  assert.equal(plain(millimetres(0.05)), "unas gotas");
  assert.equal(plain(millimetres(0.4)), "0,4 mm");
});

test("the hourly summary mentions the next rain", () => {
  const hour = 3600000;
  const now = Date.parse("2026-10-31T09:30:00+01:00");
  const start = Date.parse("2026-10-31T09:00:00+01:00");
  const hours = Array.from({ length: 12 }, (_, index) => ({
    time: start + index * hour,
    temp: 10 + index,
    rain: index === 5 ? 1.2 : 0,
    snow: 0,
    pop: 50,
    gust: null,
  }));
  const [rain, temperature] = hoursSummary(/** @type {any} */ (hours), "Europe/Madrid", now).map(plain);
  assert.equal(rain, "Lluvia prevista a las 14 h.");
  assert.equal(temperature, "Máxima de 21° a las 20 h.");
});

test("hours read naturally in Spanish", () => {
  const today = "2026-10-31";
  const zone = "Europe/Madrid";
  assert.equal(plain(relativeHour(Date.parse("2026-10-31T14:00:00+01:00"), zone, today)), "a las 14 h");
  assert.equal(plain(relativeHour(Date.parse("2026-11-01T01:00:00+01:00"), zone, today)), "mañana a la 1 h");
  assert.equal(plain(relativeHour(Date.parse("2026-11-02T09:00:00+01:00"), zone, today)), "el lunes a las 9 h");
});

test("chart curves pass through every point and never overshoot", () => {
  const points = /** @type {Array<[number, number]>} */ ([
    [0, 50],
    [1, 20],
    [2, 20],
    [3, 80],
  ]);
  const path = smoothPath(points);
  assert.match(path, /^M0,50 C/);
  for (const [x, y] of points.slice(1)) assert.ok(path.includes(`${x},${y}`));
  // Every control point stays between the values of its segment.
  const values = [...path.matchAll(/(-?[\d.]+),(-?[\d.]+)/g)].map((match) => Number(match[2]));
  assert.ok(values.every((value) => value >= 20 && value <= 80));
  assert.equal(smoothPath([[0, 1], null, [2, 3]]).split("M").length - 1, 2);
});

test("lines are softened without moving the ends or crossing gaps", () => {
  assert.deepEqual(softened([10, 10, 14, 14, null, 3]), [10, 11, 13, 14, null, 3]);
});

test("the list shows when the data was last requested from AEMET", () => {
  const now = Date.parse("2026-11-01T17:30:00+01:00");
  const zone = "Europe/Madrid";
  assert.equal(builtAt(Date.parse("2026-11-01T16:58:00+01:00"), now, zone), "Actualizado a las 16:58");
  assert.match(builtAt(Date.parse("2026-10-31T09:05:00+01:00"), now, zone), /^Actualizado el 31 oct\.?, 09:05$/);
});
