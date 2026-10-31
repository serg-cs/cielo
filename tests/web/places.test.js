import assert from "node:assert/strict";
import { test } from "node:test";

import { decodeBeaches, decodePlaces, forecastPath, normalize, search, timeZoneFor } from "../../web/scripts/places.js";
import { formatRoute, parseRoute, placeHref } from "../../web/scripts/router.js";
import { warningsFor } from "../../web/scripts/warnings.js";

const places = decodePlaces({
  schema: 4,
  provinces: [
    { code: "28", name: "Madrid", tz: "Europe/Madrid" },
    { code: "35", name: "Las Palmas", tz: "Atlantic/Canary" },
  ],
  ids: ["28079", "28058", "35016", "28005"],
  names: ["Madrid", "Fuenlabrada", "Las Palmas de Gran Canaria", "Alcalá de Henares"],
  pop: [3165235, 192000, 382283, 196000],
});

test("forecast bundles group 20 municipalities", () => {
  assert.equal(forecastPath("28079"), "v4/forecast/28/060.json");
  assert.equal(forecastPath("35016"), "v4/forecast/35/000.json");
});

test("search ignores accents and ranks prefixes then population", () => {
  assert.equal(normalize("  Alcalá  de Henares "), "alcala de henares");
  assert.deepEqual(
    search(places, "alcala").results.map((place) => place.id),
    ["28005"],
  );
  assert.deepEqual(search(places, "ma").results.map((place) => place.id)[0], "28079");
  // One letter is enough; names starting with it come first.
  assert.equal(search(places, "m").results[0]?.id, "28079");
  assert.equal(search(places, "").results.length, 0);
  assert.equal(places.get("35016")?.timeZone, "Atlantic/Canary");
});

test("search matches words in any order and ignores punctuation", () => {
  const document = {
    provinces: [
      { code: "08", name: "Barcelona" },
      { code: "28", name: "Madrid" },
    ],
    ids: ["08101", "28134"],
    names: ["L'Hospitalet de Llobregat", "San Sebastián de los Reyes"],
    pop: [270000, 90000],
  };
  const list = decodePlaces(document);
  assert.equal(search(list, "sebastian reyes").results[0]?.id, "28134");
  assert.equal(search(list, "lhospitalet").results[0]?.id, "08101");
  assert.equal(search(list, "hospitalet").results[0]?.id, "08101");
});

test("routes round-trip", () => {
  for (const hash of ["#/", "#/m/28079", "#/m/28079/d/2026-11-01", "#/m/28079/avisos"]) {
    assert.equal(formatRoute(parseRoute(hash)), hash);
  }
  assert.deepEqual(parseRoute("#/m/abc"), { view: "locations" });
});

test("warnings apply by zone or coastal municipality list and expire", () => {
  const now = Date.parse("2026-10-31T12:00:00+01:00");
  const document = {
    warnings: [
      {
        id: "a",
        level: "yellow",
        zones: ["722802"],
        expires: "2026-11-01T00:00:00+01:00",
      },
      {
        id: "b",
        level: "orange",
        zones: ["691703C"],
        m: ["28079"],
        expires: "2026-11-01T00:00:00+01:00",
      },
      {
        id: "c",
        level: "red",
        zones: ["722802"],
        expires: "2026-10-31T10:00:00+01:00",
      },
      { id: "d", level: "red", zones: ["999999"] },
    ],
  };
  assert.deepEqual(
    warningsFor(document, "28079", "722802", now).map((warning) => warning.id),
    ["b", "a"],
  );
});

test("beaches are searchable and rank after municipalities", () => {
  const document = {
    provinces: [{ code: "03", name: "Alicante" }],
    ids: ["03014", "03031"],
    names: ["Alicante/Alacant", "Benidorm"],
    pop: [330000, 70000],
    beaches: {
      ids: ["0303101", "0301401"],
      names: ["Llevant / Playa de Levante", "Sant Joan / San Juan"],
      m: ["03031", "03014"],
    },
  };
  const places = decodePlaces(document);
  const beaches = decodeBeaches(document, places);
  assert.equal(beaches.get("0303101")?.municipality, "Benidorm");
  assert.equal(beaches.get("0303101")?.timeZone, "Europe/Madrid");
  const levante = search(places, "levante", beaches).results;
  assert.deepEqual(
    levante.map((place) => place.id),
    ["0303101"],
  );
  // A municipality name finds the municipality first, then its beaches.
  const benidorm = search(places, "benidorm", beaches).results;
  assert.deepEqual(
    benidorm.map((place) => place.id),
    ["03031", "0303101"],
  );
});

test("beach routes and place links", () => {
  assert.deepEqual(parseRoute("#/p/0301401"), {
    view: "beach",
    id: "0301401",
    sheet: null,
  });
  assert.deepEqual(parseRoute("#/p/0301401/avisos"), {
    view: "beach",
    id: "0301401",
    sheet: { kind: "warnings" },
  });
  assert.deepEqual(parseRoute("#/p/030140"), { view: "locations" });
  assert.equal(formatRoute({ view: "beach", id: "0301401", sheet: null }), "#/p/0301401");
  assert.equal(placeHref("0301401"), "#/p/0301401");
  assert.equal(placeHref("28079"), "#/m/28079");
  assert.equal(timeZoneFor("38038"), "Atlantic/Canary");
  assert.equal(timeZoneFor("5100101"), "Africa/Ceuta");
});
