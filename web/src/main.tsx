import { render } from "preact";
// Registers the <esp-web-install-button> custom element.
import "esp-web-tools/dist/web/install-button.js";
import { App } from "./App";
import "./styles.css";

render(<App />, document.getElementById("app")!);
