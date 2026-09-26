import { createContext, Fragment, useContext, type ReactNode } from "react";
import type { Locale, Messages } from "./locales.ts";

interface I18n {
  locale: Locale;
  t: Messages;
}

const Context = createContext<I18n | null>(null);

export function I18nProvider({ locale, messages, children }: { locale: Locale; messages: Messages; children: ReactNode }) {
  return <Context.Provider value={{ locale, t: messages }}>{children}</Context.Provider>;
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
