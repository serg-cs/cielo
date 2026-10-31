import assert from "node:assert/strict";
import { test } from "node:test";

import { staleNote, temperatureGradient, warningTiming } from "../../web/scripts/cards.js";

test("warning timing reads as one span", () => {
  const now = Date.parse("2026-10-31T10:00:00+01:00");
  const zone = "Europe/Madrid";
  assert.equal(
    warningTiming(
      {
        id: "a",
        level: "yellow",
        zones: [],
        onset: "2026-11-01T12:00:00+01:00",
        expires: "2026-11-01T23:59:00+01:00",
      },
      zone,
      now,
    ),
    "Mañana de 12:00 a 23:59",
  );
  assert.equal(
    warningTiming(
      {
        id: "b",
        level: "yellow",
        zones: [],
        onset: "2026-10-31T08:00:00+01:00",
        expires: "2026-10-31T20:00:00+01:00",
      },
      zone,
      now,
    ),
    "En vigor hasta hoy a las 20:00",
  );
  assert.equal(
    warningTiming(
      {
        id: "c",
        level: "orange",
        zones: [],
        onset: "2026-10-31T18:00:00+01:00",
        expires: "2026-11-02T06:00:00+01:00",
      },
      zone,
      now,
    ),
    "Desde hoy a las 18:00 hasta el lunes a las 6:00",
  );
});

test("range bars map colours to fixed temperatures", () => {
  // 0° is always cyan and 20° always yellow, whatever the week's range.
  const gradient = temperatureGradient(0, 20);
  assert.match(gradient, /var\(--mono-cyan\) 0\.0%/);
  assert.match(gradient, /var\(--mono-yellow\) 100\.0%/);
  assert.match(temperatureGradient(10, 30), /var\(--mono-cyan\) -50\.0%/);
});

test("old forecasts are flagged, recent ones are not", () => {
  const now = Date.parse("2026-11-01T18:00:00+01:00");
  assert.equal(staleNote(Date.parse("2026-11-01T14:00:00+01:00"), now), null);
  assert.match(staleNote(Date.parse("2026-11-01T09:00:00+01:00"), now) ?? "", /más de 6\u00a0horas/);
  assert.equal(staleNote(0, now), null);
});
