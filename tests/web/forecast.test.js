import assert from "node:assert/strict";
import { test } from "node:test";

import {
  currentHour,
  dayParts,
  daySummary,
  decodeForecast,
  upcomingDays,
  upcomingHours,
  upcomingRisks,
} from "../../web/scripts/forecast.js";

const raw = {
  issued: "2026-10-31T09:49:07+01:00",
  meta: { zone: "722802" },
  hourly: {
    t0: "2026-10-31T07:00:00+01:00",
    sky: ["11n", null, "16", "12"],
    temp: [null, 9, 10, 12],
    feels: [null, 7, 10, 12],
    rain: [0, "Ip", 0.4, null],
    snow: [0, 0, 0, null],
    rh: [null, 84, 84, 79],
    wdir: [null, "NE", "NE", "E"],
    wind: [null, 11, 10, 9],
    gust: [null, 17, 18, 16],
  },
  windows: [{ t: "2026-10-31T07:00:00+01:00", h: 6, pop: 20, storm: 5, snow: 0 }],
  sun: { "2026-10-31": ["07:42", "18:13"] },
  daily: [
    {
      date: "2026-10-31",
      tmin: 9,
      tmax: 20,
      uv: 3,
      p: {
        "00-06": { wdir: "NE", wind: 10 },
        "06-12": { sky: "11", pop: 0 },
        "12-18": { sky: "11", pop: 10 },
        "12-24": { sky: "12", pop: 15 },
        "18-24": { sky: "11n", pop: 0 },
      },
    },
    {
      date: "2026-11-04",
      tmin: 6,
      tmax: 13,
      p: { "00-24": { sky: "15", pop: 45, snowl: 1700 } },
    },
  ],
};

const forecast = decodeForecast("28079", raw, "Europe/Madrid");

test("decodes columnar hours one absolute hour apart", () => {
  assert.ok(forecast);
  assert.equal(forecast.hours.length, 4);
  assert.equal(forecast.hours[1].time - forecast.hours[0].time, 3600 * 1000);
  assert.equal(forecast.hours[1].rain, "Ip");
  assert.equal(forecast.hours[2].pop, 20);
  assert.equal(forecast.hours[2].storm, 5);
  assert.equal(forecast.issued?.toISOString(), "2026-10-31T08:49:07.000Z");
});

test("fills a missing sky code from the previous hour", () => {
  assert.equal(forecast?.hours[1].sky, "11n");
});

test("selects the current and upcoming hours", () => {
  const now = Date.parse("2026-10-31T08:30:00+01:00");
  assert.equal(currentHour(forecast, now)?.temp, 9);
  assert.deepEqual(
    upcomingHours(forecast, now).map((hour) => hour.temp),
    [9, 10, 12],
  );
  assert.equal(upcomingRisks(forecast, now).storm, 5);
});

test("day summary ignores periods that already ended today", () => {
  const day = forecast.days[0];
  assert.deepEqual(daySummary(day, { today: "2026-10-31", hour: 13 }).sky, "12");
  assert.equal(daySummary(day, { today: "2026-10-31", hour: 19 }).period, "18-24");
  assert.equal(daySummary(forecast.days[1], { today: "2026-10-31", hour: 13 }).snowl, 1700);
});

test("day parts use quarters when available", () => {
  assert.deepEqual(
    dayParts(forecast.days[0]).map((part) => part.label),
    ["Madrugada", "Mañana", "Tarde", "Noche"],
  );
  assert.deepEqual(dayParts(forecast.days[1]), []);
});

test("upcoming days start at the local date", () => {
  const now = Date.parse("2026-11-01T00:30:00+01:00");
  assert.deepEqual(
    upcomingDays(forecast, now).map((day) => day.date),
    ["2026-11-04"],
  );
});

test("missing documents decode to null", () => {
  assert.equal(decodeForecast("28079", undefined, "Europe/Madrid"), null);
});
