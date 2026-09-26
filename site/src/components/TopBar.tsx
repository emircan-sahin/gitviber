import { useEffect, useRef } from "react";
import { useI18n } from "../i18n/index.tsx";
import { REPO_URL } from "../release.ts";
import { GitHubIcon, Logo } from "./icons.tsx";
import { LanguageMenu } from "./LanguageMenu.tsx";

/** How far down the page you are, as a line along the header's bottom edge. */
function usePageProgress() {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const update = () => {
      const max = document.documentElement.scrollHeight - innerHeight;
      el.style.setProperty("--page-p", String(max > 0 ? Math.min(1, scrollY / max) : 0));
    };
    update();
    addEventListener("scroll", update, { passive: true });
    addEventListener("resize", update);
    return () => {
      removeEventListener("scroll", update);
      removeEventListener("resize", update);
    };
  }, []);
  return ref;
}

export function TopBar() {
  const { t } = useI18n();
  const progress = usePageProgress();
  const links = [
    { href: "#agents", label: t.nav.features },
    { href: "#install", label: t.nav.download },
    { href: "#faq", label: t.nav.faq },
  ];
  return (
    <header className="fixed inset-x-0 top-0 z-40 border-b border-border/80 bg-page/70 backdrop-blur-xl backdrop-saturate-150">
      <div className="mx-auto flex h-13 max-w-6xl items-center gap-8 px-5 sm:px-8">
        <a href="#top" className="flex items-center gap-2 text-[15px] font-semibold">
          <Logo className="size-6" />
          GitViber
        </a>
        <nav aria-label={t.nav.sections} className="ml-auto hidden gap-7 text-[13px] text-muted sm:flex">
          {links.map((l) => (
            <a key={l.href} href={l.href} className="transition hover:text-fg">
              {l.label}
            </a>
          ))}
        </nav>
        <div className="-mr-2 ml-auto flex items-center gap-1 sm:ml-0">
          <LanguageMenu />
          <a
            href={REPO_URL}
            aria-label={t.nav.github}
            className="grid size-9 place-items-center rounded-md text-muted transition hover:bg-hover hover:text-fg"
          >
            <GitHubIcon className="size-[18px]" />
          </a>
        </div>
      </div>
      <div
        ref={progress}
        aria-hidden
        className="absolute inset-x-0 -bottom-px h-0.5 origin-left bg-primary shadow-[0_0_10px_rgb(74_159_245/0.7)] [transform:scaleX(var(--page-p,0))]"
      />
    </header>
  );
}
