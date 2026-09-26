import "@fontsource-variable/geist";
import "@fontsource-variable/jetbrains-mono";
import "./styles.css";
import { StrictMode } from "react";
import { createRoot, hydrateRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { I18nProvider } from "./i18n/index.tsx";
import { loadMessages, localeFromPath } from "./i18n/locales.ts";

const locale = localeFromPath(location.pathname, import.meta.env.BASE_URL);
const messages = await loadMessages(locale);
const root = document.getElementById("root")!;
const app = (
  <StrictMode>
    <I18nProvider locale={locale} messages={messages}>
      <App />
    </I18nProvider>
  </StrictMode>
);

// The build prerenders every locale (scripts/prerender.mjs); the dev server serves the bare shell.
if (import.meta.env.DEV) {
  document.documentElement.lang = locale.code;
  document.title = messages.meta.title;
  createRoot(root).render(app);
} else {
  hydrateRoot(root, app);
}
