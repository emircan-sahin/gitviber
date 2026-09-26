import { rich, useI18n } from "../i18n/index.tsx";
import { REPO_URL } from "../release.ts";

const codeClass = "rounded bg-elevated px-1.5 py-0.5 font-mono text-[0.85em] text-fg";
const link = "text-muted underline decoration-border-strong underline-offset-4 hover:text-fg";

export function Faq() {
  const { t } = useI18n();
  return (
    <section id="faq" data-section className="border-t border-border">
      <div className="mx-auto max-w-3xl px-5 py-32 sm:px-8 sm:py-40">
        <h2 className="reveal text-4xl font-semibold tracking-[-0.03em] sm:text-5xl">{t.faq.title}</h2>
        <div className="reveal mt-12 divide-y divide-border border-y border-border">
          {t.faq.items.map(({ q, a }) => (
            <details key={q} className="group">
              <summary className="flex cursor-pointer list-none items-center justify-between gap-6 py-6 transition hover:text-primary [&::-webkit-details-marker]:hidden">
                <h3 className="text-lg font-medium">{q}</h3>
                <span aria-hidden className="text-2xl leading-none font-light text-subtle transition group-open:rotate-45">
                  +
                </span>
              </summary>
              <p className="max-w-2xl pb-7 text-[17px] leading-relaxed text-muted">{rich(a, {}, codeClass)}</p>
            </details>
          ))}
        </div>
        <p className="reveal mt-8 text-[15px] text-subtle">
          {rich(t.faq.more, {
            readme: (
              <a href={`${REPO_URL}#faq`} className={link}>
                {t.faq.readme}
              </a>
            ),
          })}
        </p>
      </div>
    </section>
  );
}
