import type { CSSProperties } from "react";
import { useI18n } from "../i18n/index.tsx";

export function Numbers() {
  const { t } = useI18n();
  return (
    <section data-section className="mx-auto max-w-6xl px-5 py-32 sm:px-8 sm:py-48">
      <div className="reveal max-w-3xl text-4xl leading-[1.08] font-semibold tracking-[-0.03em] text-balance sm:text-6xl">
        <h2 className="inline">{t.numbers.title}</h2> <p className="inline text-subtle">{t.numbers.subtitle}</p>
      </div>
      <div className="mt-20 grid grid-cols-1 gap-20 sm:mt-28 md:grid-cols-2 md:gap-12">
        {t.numbers.items.map((s, i) => (
          <div key={i} className="reveal" style={{ "--delay": `${i * 150}ms` } as CSSProperties}>
            <p className="bg-linear-to-b from-fg to-fg/45 bg-clip-text text-[5.2rem] leading-none font-semibold tracking-[-0.05em] text-transparent sm:text-[8.5rem]">
              {s.big}
            </p>
            <p className="mt-6 text-2xl font-semibold tracking-tight">{s.title}</p>
            <p className="mt-3 max-w-md text-lg leading-relaxed text-pretty text-muted">{s.text}</p>
          </div>
        ))}
      </div>
    </section>
  );
}
