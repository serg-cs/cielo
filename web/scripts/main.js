import {
  ApplicationController,
} from "./application-controller.js";
import {
  requiredElement,
} from "./dom.js";

const root = requiredElement(
  document.querySelector("#cielo-application"),
  HTMLElement,
);
const application = new ApplicationController(root);
application.start();
