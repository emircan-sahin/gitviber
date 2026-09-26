import type { CSSProperties } from "react";
import { useI18n } from "../i18n/index.tsx";

export function Rest() {
  const { t } = useI18n();
  return (
    <section data-section className="border-t border-border">
      <div className="mx-auto max-w-6xl px-5 py-32 sm:px-8 sm:py-44">
        <div className="reveal max-w-3xl text-4xl leading-[1.08] font-semibold tracking-[-0.03em] text-balance sm:text-6xl">
          <h2 className="inline">{t.rest.title}</h2> <p className="inline text-subtle">{t.rest.subtitle}</p>
        </div>
        <dl className="mt-20 grid grid-cols-1 gap-x-12 gap-y-12 sm:mt-24 sm:grid-cols-2 lg:grid-cols-3">
          {t.rest.items.map((item, i) => (
            <div
              key={i}
              className="reveal border-t border-border-strong pt-6"
              style={{ "--delay": `${(i % 3) * 100}ms` } as CSSProperties}
            >
              <dt className="text-xl font-semibold tracking-tight">{item.title}</dt>
              <dd className="mt-2 text-[17px] leading-relaxed text-muted">{item.text}</dd>
            </div>
          ))}
        </dl>
      </div>
    </section>
  );
}
