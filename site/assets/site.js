import "./components/cielo-icon.js";
import "./components/cielo-app.js";

if ("serviceWorker" in navigator) {
  const serviceWorkerUrl = new URL("../service-worker.js", import.meta.url);
  navigator.serviceWorker.register(serviceWorkerUrl).catch((error) => {
    console.error("No se pudo activar el funcionamiento sin conexión", error);
  });
}
