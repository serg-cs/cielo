import assert from "node:assert/strict";
import { test } from "node:test";

import { fireDanger, sky, uvCategory, warningPhenomenon, windDirection } from "../../web/scripts/codes.js";

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
