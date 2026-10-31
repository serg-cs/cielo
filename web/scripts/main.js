import { App } from "./app.js";

const root = document.getElementById("cielo-application");
if (root) {
  void new App(root).start();
}

// Cache the app itself so it opens without a connection.
if ("serviceWorker" in navigator && location.protocol !== "file:") {
  window.addEventListener("load", () => {
    // The worker lives next to index.html, not next to this module.
    const worker = new URL("sw.js", document.baseURI);
    navigator.serviceWorker.register(worker, { scope: new URL(".", document.baseURI).pathname }).catch((error) => {
      console.warn("Cielo: service worker registration failed", error);
    });
  });
}
