export function requiredElement(element, constructor) {
  if (!(element instanceof constructor)) {
    throw new Error("La estructura de la aplicación no es válida");
  }
  return element;
}

export function setDynamicIcon(icon, name) {
  const use = icon.querySelector("use");
  if (use === null) {
    throw new Error("La estructura del icono no es válida");
  }

  if (name === null) {
    use.removeAttribute("href");
  } else {
    use.setAttribute("href", `#cielo-icon-${name}`);
  }
  icon.toggleAttribute("hidden", name === null);
}
