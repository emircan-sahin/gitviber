import { createContext, Fragment, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { loadMessages, localeFromPath, type Locale, type Messages } from "./locales.ts";

interface I18n {
  locale: Locale;
  t: Messages;
  /** Swaps the page's language in place and moves the URL to that locale's page. */
  switchTo: (locale: Locale) => void;
}

const Context = createContext<I18n | null>(null);

export function I18nProvider({ locale, messages, children }: { locale: Locale; messages: Messages; children: ReactNode }) {
  const [current, setCurrent] = useState({ locale, t: messages });
  const code = useRef(locale.code);

  const show = useCallback(async (next: Locale) => {
    if (next.code === code.current) return;
    code.current = next.code;
    const t = await loadMessages(next);
    // A quicker second pick wins.
    if (code.current !== next.code) return;
    setCurrent({ locale: next, t });
    document.documentElement.lang = next.code;
    document.title = t.meta.title;
  }, []);

  const switchTo = useCallback(
    (next: Locale) => {
      if (next.code === code.current) return;
      history.pushState(null, "", `${import.meta.env.BASE_URL}${next.path}${location.hash}`);
      void show(next);
    },
    [show],
  );

  // Back and forward move between the locales' pages too.
  useEffect(() => {
    const onPop = () => void show(localeFromPath(location.pathname, import.meta.env.BASE_URL));
    addEventListener("popstate", onPop);
    return () => removeEventListener("popstate", onPop);
  }, [show]);

  return <Context.Provider value={{ ...current, switchTo }}>{children}</Context.Provider>;
}

export function useI18n() {
  const value = useContext(Context);
  if (!value) throw new Error("useI18n outside I18nProvider");
  return value;
}

/** "{name}" filled from vars, for plain strings (attributes, titles). */
export const fill = (text: string, vars: Record<string, string | number>) =>
  text.replace(/\{(\w+)\}/g, (_, key: string) => String(vars[key] ?? ""));

/**
 * A message with markup: `code` becomes <code>, "{name}" becomes slots[name]. Translators move
 * both around freely, so word order stays theirs.
 */
export function rich(text: string, slots: Record<string, ReactNode> = {}, codeClass = "font-mono text-fg") {
  return text.split(/(\{\w+\}|`[^`]+`)/).map((part, i) => {
    if (/^\{\w+\}$/.test(part)) return <Fragment key={i}>{slots[part.slice(1, -1)]}</Fragment>;
    if (part.startsWith("`")) {
      return (
        <code key={i} className={codeClass}>
          {part.slice(1, -1)}
        </code>
      );
    }
    return part;
  });
}
