const collator = new Intl.Collator("es", {
  numeric: true,
  sensitivity: "base",
});
const maximumSearchResults = 50;
const minimumSearchLength = 2;
const storageKey = "cielo.trackedMunicipalities";

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

function normalizeSearchText(value) {
  return value
    .normalize("NFD")
    .replace(/\p{M}/gu, "")
    .toLocaleLowerCase("es")
    .trim()
    .replace(/\s+/gu, " ");
}

function createMunicipalitySummary(municipality) {
  const summary = document.createElement("div");
  const name = document.createElement("span");
  const province = document.createElement("span");

  summary.className = "municipality-summary";
  name.className = "municipality-name";
  name.textContent = municipality.name;
  province.className = "municipality-province";
  province.textContent = municipality.province;
  summary.append(name, province);

  return summary;
}

function readTrackedMunicipalityIds() {
  return new Set(JSON.parse(localStorage.getItem(storageKey) ?? "[]"));
}

function saveTrackedMunicipalityIds(trackedIds) {
  localStorage.setItem(storageKey, JSON.stringify([...trackedIds]));
}

function renderTrackedMunicipalities(elements, municipalitiesById, trackedIds) {
  const trackedMunicipalities = [...trackedIds]
    .map((id) => municipalitiesById.get(id))
    .filter((municipality) => municipality !== undefined)
    .sort(compareMunicipalities);
  const fragment = document.createDocumentFragment();

  for (const municipality of trackedMunicipalities) {
    const item = document.createElement("li");
    const removeButton = document.createElement("button");

    removeButton.className = "remove-button";
    removeButton.type = "button";
    removeButton.textContent = "Quitar";
    removeButton.setAttribute(
      "aria-label",
      `Dejar de seguir ${municipality.name}, ${municipality.province}`,
    );
    removeButton.addEventListener("click", () => {
      trackedIds.delete(municipality.id);
      saveTrackedMunicipalityIds(trackedIds);
      renderTrackedMunicipalities(elements, municipalitiesById, trackedIds);
    });
    item.append(createMunicipalitySummary(municipality), removeButton);
    fragment.append(item);
  }

  elements.list.replaceChildren(fragment);
  elements.emptyState.hidden = trackedMunicipalities.length > 0;
  elements.list.hidden = trackedMunicipalities.length === 0;
}

function findMunicipalities(municipalities, query) {
  const matches = municipalities
    .filter((municipality) => municipality.searchName.includes(query))
    .sort((left, right) => {
      const prefixDifference =
        Number(right.searchName.startsWith(query)) -
        Number(left.searchName.startsWith(query));
      return prefixDifference || compareMunicipalities(left, right);
    });

  return {
    municipalities: matches.slice(0, maximumSearchResults),
    total: matches.length,
  };
}

function renderSearchResults(
  elements,
  municipalities,
  municipalitiesById,
  trackedIds,
) {
  const query = normalizeSearchText(elements.searchInput.value);
  if (query.length < minimumSearchLength) {
    elements.searchStatus.textContent = "Escribe al menos 2 caracteres.";
    elements.searchResults.replaceChildren();
    return;
  }

  const result = findMunicipalities(municipalities, query);
  const fragment = document.createDocumentFragment();

  for (const municipality of result.municipalities) {
    const item = document.createElement("li");
    const followButton = document.createElement("button");
    const alreadyTracked = trackedIds.has(municipality.id);

    followButton.type = "button";
    followButton.disabled = alreadyTracked;
    followButton.textContent = alreadyTracked ? "Ya añadido" : "Seguir";
    followButton.setAttribute(
      "aria-label",
      alreadyTracked
        ? `${municipality.name}, ${municipality.province}, ya está añadido`
        : `Seguir ${municipality.name}, ${municipality.province}`,
    );
    followButton.addEventListener("click", () => {
      trackedIds.add(municipality.id);
      saveTrackedMunicipalityIds(trackedIds);
      renderTrackedMunicipalities(elements, municipalitiesById, trackedIds);
      elements.searchDialog.close();
    });
    item.append(createMunicipalitySummary(municipality), followButton);
    fragment.append(item);
  }

  elements.searchResults.replaceChildren(fragment);
  if (result.total === 0) {
    elements.searchStatus.textContent = "No se encontraron municipios.";
  } else if (result.total > maximumSearchResults) {
    elements.searchStatus.textContent =
      `Mostrando ${maximumSearchResults} de ${result.total} resultados. ` +
      "Escribe algo más para precisar la búsqueda.";
  } else {
    elements.searchStatus.textContent =
      result.total === 1 ? "1 resultado." : `${result.total} resultados.`;
  }
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
  const emptyState = document.querySelector("#empty-state");
  const searchButton = document.querySelector("#search-button");
  const searchDialog = document.querySelector("#search-dialog");
  const closeSearch = document.querySelector("#close-search");
  const searchInput = document.querySelector("#municipality-search");
  const searchStatus = document.querySelector("#search-status");
  const searchResults = document.querySelector("#search-results");
  if (
    !(list instanceof HTMLUListElement) ||
    !(status instanceof HTMLParagraphElement) ||
    !(emptyState instanceof HTMLParagraphElement) ||
    !(searchButton instanceof HTMLButtonElement) ||
    !(searchDialog instanceof HTMLDialogElement) ||
    !(closeSearch instanceof HTMLButtonElement) ||
    !(searchInput instanceof HTMLInputElement) ||
    !(searchStatus instanceof HTMLParagraphElement) ||
    !(searchResults instanceof HTMLUListElement)
  ) {
    throw new Error("La página no contiene los elementos esperados");
  }

  const elements = {
    list,
    emptyState,
    searchButton,
    searchDialog,
    searchInput,
    searchStatus,
    searchResults,
  };

  try {
    const municipalities = await loadMunicipalities();
    const searchableMunicipalities = municipalities.map((municipality) => ({
      ...municipality,
      searchName: normalizeSearchText(municipality.name),
    }));
    const municipalitiesById = new Map(
      searchableMunicipalities.map((municipality) => [municipality.id, municipality]),
    );
    const trackedIds = readTrackedMunicipalityIds();

    // Wire the picker only after its catalog is ready to use.
    searchButton.addEventListener("click", () => searchDialog.showModal());
    closeSearch.addEventListener("click", () => searchDialog.close());
    searchInput.addEventListener("input", () => {
      renderSearchResults(
        elements,
        searchableMunicipalities,
        municipalitiesById,
        trackedIds,
      );
    });
    searchDialog.addEventListener("close", () => {
      searchInput.value = "";
      searchStatus.textContent = "Escribe al menos 2 caracteres.";
      searchResults.replaceChildren();
    });

    renderTrackedMunicipalities(elements, municipalitiesById, trackedIds);
    searchButton.disabled = false;
    status.hidden = true;
  } catch (error) {
    console.error(error);
    status.setAttribute("role", "alert");
    status.textContent = "No se pudieron cargar los municipios.";
  }
}

await initialize();
