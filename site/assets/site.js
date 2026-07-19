const collator = new Intl.Collator("es", {
  numeric: true,
  sensitivity: "base",
});

function compareMunicipalities(left, right) {
  return (
    collator.compare(left.name, right.name) ||
    collator.compare(left.province, right.province) ||
    collator.compare(left.id, right.id)
  );
}

function validateMunicipalities(document) {
  if (
    typeof document !== "object" ||
    document === null ||
    !Array.isArray(document.municipalities) ||
    !document.municipalities.every(
      (municipality) =>
        typeof municipality === "object" &&
        municipality !== null &&
        typeof municipality.id === "string" &&
        typeof municipality.name === "string" &&
        typeof municipality.province === "string",
    )
  ) {
    throw new Error("El documento de municipios no es válido");
  }

  return document.municipalities;
}

function renderMunicipalities(list, municipalities) {
  const fragment = document.createDocumentFragment();

  for (const municipality of [...municipalities].sort(compareMunicipalities)) {
    const item = document.createElement("li");
    const name = document.createElement("span");
    const province = document.createElement("span");

    name.className = "municipality-name";
    name.textContent = municipality.name;
    province.className = "municipality-province";
    province.textContent = `— ${municipality.province}`;
    item.append(name, province);
    fragment.append(item);
  }

  list.replaceChildren(fragment);
}

async function loadMunicipalities() {
  const response = await fetch("./data/municipalities.json");
  if (!response.ok) {
    throw new Error(`No se pudieron cargar los municipios: HTTP ${response.status}`);
  }

  return validateMunicipalities(await response.json());
}

async function initialize() {
  const list = document.querySelector("#municipality-list");
  const status = document.querySelector("#status");
  if (!(list instanceof HTMLUListElement) || !(status instanceof HTMLParagraphElement)) {
    throw new Error("La página no contiene los elementos esperados");
  }

  try {
    const municipalities = await loadMunicipalities();
    renderMunicipalities(list, municipalities);
    status.hidden = true;
  } catch (error) {
    console.error(error);
    status.role = "alert";
    status.textContent = "No se pudieron cargar los municipios.";
  }
}

await initialize();
