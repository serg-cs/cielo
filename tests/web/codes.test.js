import assert from "node:assert/strict";
import { test } from "node:test";

import {
  WIND_LEVEL_LIMITS,
  fireDanger,
  sky,
  uvCategory,
  warningPhenomenon,
  windDirection,
  windLevel,
  windStrength,
} from "../../web/scripts/codes.js";

test("every AEMET sky code has an icon and a Spanish label", () => {
  const codes = [
    11, 12, 13, 14, 15, 16, 17, 23, 24, 25, 26, 33, 34, 35, 36, 43, 44, 45, 46, 51, 52, 53, 54, 61, 62, 63, 64, 71, 72,
    73, 74, 81, 82, 83,
  ];
  for (const code of codes) {
    for (const suffix of ["", "n"]) {
      const condition = sky(`${code}${suffix}`);
      assert.ok(condition.icon, `${code}${suffix} has an icon`);
      assert.notEqual(condition.label, "Estado del cielo desconocido", `${code}${suffix} has a label`);
    }
  }
});

test("night codes use night icons where one exists", () => {
  assert.equal(sky("11").icon, "sun");
  assert.equal(sky("11n").icon, "moon");
  assert.equal(sky("12n").icon, "cloud-moon");
  assert.equal(sky("43n").icon, "cloud-moon-rain");
  assert.equal(sky("11n").night, true);
});

test("unknown and missing codes are tolerated", () => {
  assert.equal(sky("99").label, "Estado del cielo desconocido");
  assert.equal(sky(null).label, "Sin datos");
});

test("wind directions", () => {
  assert.equal(windDirection("SO").degrees, 225);
  assert.equal(windDirection("C").label, "calma");
  assert.equal(windDirection("VRB").degrees, null);
  assert.equal(windDirection(undefined).label, "");
});

test("wind colour levels follow AEMET's wind terms, with calm on its own", () => {
  assert.equal(windLevel(null), null);
  // Calma, flojo, moderado, fuerte, muy fuerte, huracanado.
  const speeds = [0, 5, 6, 20, 21, 40, 41, 70, 71, 120, 121];
  assert.deepEqual(
    speeds.map((speed) => windLevel(speed)),
    [0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5],
  );
  assert.deepEqual(
    speeds.map((speed) => windStrength(speed)),
    [
      "Calma",
      "Calma",
      "Flojo",
      "Flojo",
      "Moderado",
      "Moderado",
      "Fuerte",
      "Fuerte",
      "Muy fuerte",
      "Muy fuerte",
      "Huracanado",
    ],
  );
  // Each limit is the first speed of the next level.
  WIND_LEVEL_LIMITS.forEach((limit, index) => {
    assert.equal(windLevel(limit), index + 1);
    assert.equal(windLevel(limit - 1), index);
  });
});

test("UV and fire categories", () => {
  assert.equal(uvCategory(2)?.label, "Bajo");
  assert.equal(uvCategory(6)?.label, "Alto");
  assert.equal(uvCategory(11)?.label, "Extremo");
  assert.equal(uvCategory(null), null);
  assert.equal(fireDanger(4)?.label, "Alto");
  assert.equal(fireDanger(0), null);
});

test("warnings show their name with an icon by code or name", () => {
  assert.deepEqual(warningPhenomenon({ code: "PR", name: "Lluvias" }), {
    name: "Lluvias",
    icon: "cloud-rain",
  });
  assert.equal(warningPhenomenon({ code: "TO", name: "Tormentas" }).icon, "cloud-lightning");
  assert.equal(warningPhenomenon({ code: "XX", name: "Costeros" }).icon, "waves");
  assert.deepEqual(warningPhenomenon({}), {
    name: "Aviso",
    icon: "triangle-alert",
  });
});
