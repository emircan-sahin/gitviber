import { Star } from "lucide-react";
import { useEffect, useRef } from "react";
import { formatStars, useStars } from "../hooks/useStars.ts";
import { rich, useI18n } from "../i18n/index.tsx";
import { AUTHOR, REPO_URL } from "../release.ts";
import { GitHubIcon, LinkedInIcon, Logo, XIcon } from "./icons.tsx";
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

const iconLink = "grid size-9 place-items-center rounded-md text-muted transition hover:bg-hover hover:text-fg";

/** The repo link, with its live star count once there is one. */
function Stars() {
  const { t } = useI18n();
  const stars = useStars();
  if (stars === null) {
    return (
      <a href={REPO_URL} aria-label={t.nav.github} className={iconLink}>
        <GitHubIcon className="size-[18px]" />
      </a>
    );
  }
  const count = <b className="font-semibold text-fg">{formatStars(stars)}</b>;
  return (
    <a
      href={REPO_URL}
      aria-label={t.nav.github}
      className="mr-1 flex h-8 items-center gap-1.5 rounded-full border border-border-strong bg-panel px-3 text-[13px] text-muted transition hover:border-subtle hover:text-fg"
    >
      <Star className="size-3.5 fill-modified text-modified" />
      <span className="hidden sm:inline">{rich(t.nav.stars, { count })}</span>
      <span className="sm:hidden">{count}</span>
    </a>
  );
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
      <div className="mx-auto flex h-13 max-w-6xl items-center gap-6 px-5 sm:px-8">
        <a href="#top" className="flex items-center gap-2 text-[15px] font-semibold">
          <Logo className="size-6" />
          GitViber
        </a>
        <span aria-hidden className="hidden h-5 w-px bg-border-strong sm:block" />
        <nav aria-label={t.nav.sections} className="hidden gap-6 text-[13px] text-muted sm:flex">
          {links.map((l) => (
            <a key={l.href} href={l.href} className="transition hover:text-fg">
              {l.label}
            </a>
          ))}
        </nav>
        <div className="-mr-2 ml-auto flex items-center gap-1">
          <Stars />
          <a href={AUTHOR.x} aria-label={t.nav.x} className={`${iconLink} max-sm:hidden`}>
            <XIcon className="size-[15px]" />
          </a>
          <a href={AUTHOR.linkedin} aria-label={t.nav.linkedin} className={`${iconLink} max-sm:hidden`}>
            <LinkedInIcon className="size-[16px]" />
          </a>
          <LanguageMenu />
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
