/**
 * Meaning of AEMET codes: sky states, wind, beaches, warnings and indices.
 */

const CLOUD_LEVELS = ["Intervalos nubosos", "Nuboso", "Muy nuboso", "Cubierto"];

/** @type {Record<string, string>} */
const SKY_LABELS = {
  11: "Despejado",
  12: "Poco nuboso",
  13: "Intervalos nubosos",
  14: "Nuboso",
  15: "Muy nuboso",
  16: "Cubierto",
  17: "Nubes altas",
  81: "Niebla",
  82: "Bruma",
  83: "Calima",
};
for (const [base, suffix] of [
  [23, "con lluvia"],
  [33, "con nieve"],
  [43, "con lluvia escasa"],
  [51, "con tormenta"],
  [61, "con tormenta y lluvia escasa"],
  [71, "con nieve escasa"],
]) {
  CLOUD_LEVELS.forEach((level, index) => {
    SKY_LABELS[base + index] = `${level} ${suffix}`;
  });
}

/**
 * Icon and label for an AEMET sky code such as `13` or `13n`.
 *
 * @param {string | null | undefined} code
 * @returns {{icon: string, label: string, night: boolean}}
 */
export function sky(code) {
  if (!code) {
    return { icon: "cloud", label: "Sin datos", night: false };
  }
  const night = code.endsWith("n");
  const value = Number.parseInt(code, 10);
  const label = SKY_LABELS[value] ?? "Estado del cielo desconocido";
  const tens = Math.floor(value / 10);
  let icon = "cloud";
  if (value === 11) {
    icon = night ? "moon" : "sun";
  } else if (value === 12 || value === 13 || value === 17) {
    icon = night ? "cloud-moon" : "cloud-sun";
  } else if (value === 14) {
    icon = "cloud";
  } else if (value === 15 || value === 16) {
    icon = "cloudy";
  } else if (tens === 2) {
    icon = value === 23 ? (night ? "cloud-moon-rain" : "cloud-sun-rain") : "cloud-rain";
  } else if (tens === 4) {
    icon = value === 43 ? (night ? "cloud-moon-rain" : "cloud-sun-rain") : "cloud-drizzle";
  } else if (tens === 3) {
    icon = "snowflake";
  } else if (tens === 7) {
    icon = "cloud-snow";
  } else if (tens === 5 || tens === 6) {
    icon = "cloud-lightning";
  } else if (value === 81 || value === 82) {
    icon = "cloud-fog";
  } else if (value === 83) {
    icon = "haze";
  }
  return { icon, label, night };
}

/** @type {Record<string, {degrees: number, label: string, short: string}>} */
const WIND_DIRECTIONS = {
  N: { degrees: 0, label: "norte", short: "N" },
  NE: { degrees: 45, label: "nordeste", short: "NE" },
  E: { degrees: 90, label: "este", short: "E" },
  SE: { degrees: 135, label: "sudeste", short: "SE" },
  S: { degrees: 180, label: "sur", short: "S" },
  SO: { degrees: 225, label: "suroeste", short: "SO" },
  O: { degrees: 270, label: "oeste", short: "O" },
  NO: { degrees: 315, label: "noroeste", short: "NO" },
};

/**
 * Where the wind comes from.
 *
 * @param {string | null | undefined} code AEMET direction: `N`, `SO`, `C` (calm), `VRB`.
 * @returns {{degrees: number | null, label: string, short: string}}
 */
export function windDirection(code) {
  if (code === "C") {
    return { degrees: null, label: "calma", short: "Calma" };
  }
  if (code === "VRB") {
    return { degrees: null, label: "variable", short: "Var." };
  }
  return WIND_DIRECTIONS[code ?? ""] ?? { degrees: null, label: "", short: "" };
}

/**
 * Beaufort-like description of a wind speed in km/h.
 *
 * @param {number | null | undefined} speed
 */
export function windStrength(speed) {
  if (speed === null || speed === undefined) {
    return "";
  }
  // AEMET's wind terms.
  if (speed < 6) return "Calma";
  if (speed <= 20) return "Flojo";
  if (speed <= 40) return "Moderado";
  if (speed <= 70) return "Fuerte";
  if (speed <= 120) return "Muy fuerte";
  return "Huracanado";
}

/** @type {Record<number, {icon: string, label: string}>} */
const BEACH_SKY = {
  100: { icon: "sun", label: "Despejado" },
  110: { icon: "cloud-sun", label: "Nuboso" },
  120: { icon: "cloudy", label: "Muy nuboso" },
  130: { icon: "cloud-sun-rain", label: "Chubascos" },
  140: { icon: "cloud-rain", label: "Muy nuboso con lluvia" },
};

/**
 * Sky at a beach, from AEMET's beach code (100-140).
 *
 * @param {number | null | undefined} code
 */
export function beachSky(code) {
  return BEACH_SKY[code ?? 0] ?? { icon: "cloud", label: "Sin datos" };
}

/**
 * Three-step beach scales: wind (210-230) and waves (310-330). The level
 * (1-3) drives a calm/caution/rough colour.
 *
 * @type {Record<number, {label: string, level: number}>}
 */
const BEACH_LEVELS = {
  210: { label: "Flojo", level: 1 },
  220: { label: "Moderado", level: 2 },
  230: { label: "Fuerte", level: 3 },
  310: { label: "Débil", level: 1 },
  320: { label: "Moderado", level: 2 },
  330: { label: "Fuerte", level: 3 },
};

/** @param {number | null | undefined} code */
export const beachWind = (code) => BEACH_LEVELS[code ?? 0] ?? null;
/** @param {number | null | undefined} code */
export const beachWaves = (code) => BEACH_LEVELS[code ?? 0] ?? null;

/**
 * UV index category.
 *
 * @param {number | null | undefined} index
 * @returns {{label: string, level: number, advice: string} | null}
 */
export function uvCategory(index) {
  if (index === null || index === undefined) return null;
  if (index <= 2) return { label: "Bajo", level: 1, advice: "No hace falta protección" };
  if (index <= 5)
    return {
      label: "Moderado",
      level: 2,
      advice: "Usa protección solar a mediodía",
    };
  if (index <= 7)
    return {
      label: "Alto",
      level: 3,
      advice: "Protección necesaria; busca sombra a mediodía",
    };
  if (index <= 10)
    return {
      label: "Muy alto",
      level: 4,
      advice: "Protección extra; evita el sol a mediodía",
    };
  return {
    label: "Extremo",
    level: 5,
    advice: "Evita el sol en las horas centrales",
  };
}

const FIRE_LABELS = ["", "Muy bajo", "Bajo", "Moderado", "Alto", "Muy alto", "Extremo"];

/**
 * Wildfire danger level (1-6).
 *
 * @param {number | null | undefined} level
 */
export function fireDanger(level) {
  if (!level || level < 1 || level > 6) return null;
  return { label: FIRE_LABELS[level], level };
}

/**
 * Wind strength on the severity scale, for colouring: 0 calm, 1 flojo,
 * 2 moderado, 3 fuerte, 4 muy fuerte, 5 huracanado. `null` without data.
 *
 * @param {number | null | undefined} speed km/h
 * @returns {number | null}
 */
export function windLevel(speed) {
  if (speed === null || speed === undefined) return null;
  if (speed < 6) return 0;
  if (speed <= 20) return 1;
  if (speed <= 40) return 2;
  if (speed <= 70) return 3;
  if (speed <= 120) return 4;
  return 5;
}

/** Speeds (km/h) where the wind changes level, for colouring charts. */
export const WIND_LEVEL_LIMITS = [6, 21, 41, 71, 121];

/** @type {Record<string, {label: string, meaning: string, rank: number}>} */
export const WARNING_LEVELS = {
  yellow: {
    label: "Amarillo",
    meaning: "Riesgo para actividades concretas",
    rank: 1,
  },
  orange: { label: "Naranja", meaning: "Riesgo importante", rank: 2 },
  red: { label: "Rojo", meaning: "Riesgo extraordinario", rank: 3 },
};

/** Icons by Meteoalerta phenomenon code. */
const PHENOMENA = {
  TO: "cloud-lightning",
  PR: "cloud-rain",
  NE: "snowflake",
  AV: "snowflake",
  CO: "waves",
  GL: "waves",
  RI: "waves",
  VI: "wind",
  AT: "thermometer",
  BT: "thermometer",
  FG: "cloud-fog",
  DU: "haze",
};

/** Icons by name, for codes not listed above. */
const PHENOMENA_BY_NAME = [
  { pattern: /tormenta/i, icon: "cloud-lightning" },
  { pattern: /lluvia|precipitaci/i, icon: "cloud-rain" },
  { pattern: /nev|nieve|alud/i, icon: "snowflake" },
  { pattern: /costero|galerna|rissaga|oleaje/i, icon: "waves" },
  { pattern: /viento/i, icon: "wind" },
  { pattern: /temperatura|calor|fr[ií]o/i, icon: "thermometer" },
  { pattern: /niebla/i, icon: "cloud-fog" },
  { pattern: /polvo|calima/i, icon: "haze" },
];

/**
 * Display name and icon of a warning.
 *
 * @param {{code?: string, name?: string}} warning
 * @returns {{name: string, icon: string}}
 */
export function warningPhenomenon(warning) {
  const name = warning.name || "Aviso";
  const icon =
    PHENOMENA[/** @type {keyof typeof PHENOMENA} */ (warning.code ?? "")] ??
    PHENOMENA_BY_NAME.find(({ pattern }) => pattern.test(name))?.icon ??
    "triangle-alert";
  return { name, icon };
}
