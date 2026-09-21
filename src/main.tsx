import "@fontsource-variable/geist";
import "@fontsource-variable/geist-mono";
import "@fontsource-variable/jetbrains-mono";
import "./index.css";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";

import { Fixture } from "./dev-fixture";

createRoot(document.getElementById("root")!).render(
  <StrictMode>{import.meta.env.DEV && location.search.includes("fixture") ? <Fixture /> : <App />}</StrictMode>,
);

