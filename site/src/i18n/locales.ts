import type en from "./messages/en.json";

export type Messages = typeof en;

export interface Locale {
  /** BCP 47 tag: <html lang>, hreflang, the messages file's name. */
  code: string;
  /** Where the locale lives under the site's base, "" for English at the root. */
  path: string;
  /** The language's own name, for the switcher. */
  name: string;
  /** Open Graph locale. */
  og: string;
}

// A locale is published only once its messages file exists (scripts/check-locales.mjs keeps them
// complete), so a half-done translation never gets a URL or an hreflang.
const ALL: Locale[] = [
  { code: "en", path: "", name: "English", og: "en_US" },
  { code: "tr", path: "tr/", name: "Türkçe", og: "tr_TR" },
  { code: "de", path: "de/", name: "Deutsch", og: "de_DE" },
  { code: "fr", path: "fr/", name: "Français", og: "fr_FR" },
  { code: "es", path: "es/", name: "Español", og: "es_ES" },
  { code: "pt-BR", path: "pt-br/", name: "Português (Brasil)", og: "pt_BR" },
  { code: "ja", path: "ja/", name: "日本語", og: "ja_JP" },
  { code: "zh-CN", path: "zh-cn/", name: "简体中文", og: "zh_CN" },
];

const files = import.meta.glob<Messages>("./messages/*.json", { import: "default" });
const has = (code: string) => `./messages/${code}.json` in files;

export const LOCALES = ALL.filter((l) => has(l.code));
export const DEFAULT_LOCALE = ALL[0];

export const loadMessages = (locale: Locale) => files[`./messages/${locale.code}.json`]();

/** The locale a path under the site's base belongs to. */
export function localeFromPath(pathname: string, base: string): Locale {
  const rest = pathname.startsWith(base) ? pathname.slice(base.length) : "";
  return LOCALES.find((l) => l.path && rest.startsWith(l.path)) ?? DEFAULT_LOCALE;
}
