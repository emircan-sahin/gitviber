import { Check, Globe } from "lucide-react";
import { useEffect, useRef } from "react";
import { useI18n } from "../i18n/index.tsx";
import { LOCALES } from "../i18n/locales.ts";
import { cx } from "./ui.tsx";

/**
 * Links to the other languages' pages, in the app's dropdown style. `up` opens it above. A plain
 * click swaps the language in place; the links stay real for crawlers and new tabs.
 */
export function LanguageMenu({ up }: { up?: boolean }) {
  const { locale, t, switchTo } = useI18n();
  const ref = useRef<HTMLDetailsElement>(null);

  // <details> doesn't close on its own; an outside click or Escape does here.
  useEffect(() => {
    const onPointer = (e: PointerEvent) => {
      const el = ref.current;
      if (el?.open && !el.contains(e.target as Node)) el.open = false;
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && ref.current?.open) ref.current.open = false;
    };
    addEventListener("pointerdown", onPointer);
    addEventListener("keydown", onKey);
    return () => {
      removeEventListener("pointerdown", onPointer);
      removeEventListener("keydown", onKey);
    };
  }, []);

  if (LOCALES.length < 2) return null;
  return (
    <details ref={ref} className="relative">
      <summary
        aria-label={t.nav.language}
        className="flex h-9 cursor-pointer list-none items-center gap-1.5 rounded-md px-2 text-[13px] text-muted transition hover:bg-hover hover:text-fg [&::-webkit-details-marker]:hidden"
      >
        <Globe className="size-4" />
        <span>{locale.name}</span>
      </summary>
      <div
        className={cx(
          "absolute right-0 z-50 min-w-48 rounded-md border border-border-strong bg-elevated p-1 shadow-lg shadow-black/50",
          up ? "bottom-full mb-1" : "top-full mt-1",
        )}
      >
        {LOCALES.map((l) => (
          <a
            key={l.code}
            href={`${import.meta.env.BASE_URL}${l.path}`}
            hrefLang={l.code}
            lang={l.code}
            aria-current={l.code === locale.code ? "page" : undefined}
            onClick={(e) => {
              if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey || e.button !== 0) return;
              e.preventDefault();
              if (ref.current) ref.current.open = false;
              switchTo(l);
            }}
            className="flex items-center gap-2 rounded-sm px-2 py-1.5 text-[13px] text-fg hover:bg-primary hover:text-white"
          >
            <Check className={cx("size-3.5", l.code !== locale.code && "invisible")} />
            {l.name}
          </a>
        ))}
      </div>
    </details>
  );
}
