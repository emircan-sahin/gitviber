import { StrictMode } from "react";
import { renderToString } from "react-dom/server";
import { App } from "./App.tsx";
import { ogImagePath, pageUrl, renderHead } from "./head.ts";
import { I18nProvider } from "./i18n/index.tsx";
import { LOCALES, type Messages } from "./i18n/locales.ts";

const messages = import.meta.glob<Messages>("./i18n/messages/*.json", { import: "default", eager: true });

export { LOCALES, ogImagePath, pageUrl };
export { BREW, REPO_URL, siteUrl, version } from "./release.ts";

/** One locale's page: its <body> markup and its <head> tags. */
export function render(code: string) {
  const locale = LOCALES.find((l) => l.code === code);
  if (!locale) throw new Error(`no locale ${code}`);
  const t = messages[`./i18n/messages/${code}.json`];
  const html = renderToString(
    <StrictMode>
      <I18nProvider locale={locale} messages={t}>
        <App />
      </I18nProvider>
    </StrictMode>,
  );
  return { html, head: renderHead(locale, t) };
}
