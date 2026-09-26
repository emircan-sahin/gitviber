import { rich, useI18n } from "../i18n/index.tsx";
import { release, REPO_URL } from "../release.ts";
import { LanguageMenu } from "./LanguageMenu.tsx";

export function Footer() {
  const { t } = useI18n();
  const links = [
    { href: REPO_URL, label: t.footer.source },
    { href: release.url, label: t.footer.releases },
    { href: `${REPO_URL}/blob/main/CHANGELOG.md`, label: t.footer.changelog },
    { href: `${REPO_URL}/blob/main/CONTRIBUTING.md`, label: t.footer.contributing },
    { href: `${REPO_URL}/security/policy`, label: t.footer.security },
  ];
  return (
    <footer className="border-t border-border">
      <div className="mx-auto flex max-w-6xl flex-col gap-5 px-5 py-10 text-[13px] text-subtle sm:flex-row sm:items-center sm:justify-between sm:px-8">
        <p>
          {rich(t.footer.made, {
            author: (
              <a href="https://github.com/emircan-sahin" className="text-muted transition hover:text-fg">
                Emircan Sahin
              </a>
            ),
          })}
        </p>
        <nav aria-label={t.footer.project} className="flex flex-wrap items-center gap-x-6 gap-y-3">
          {links.map((l) => (
            <a key={l.label} href={l.href} className="transition hover:text-fg">
              {l.label}
            </a>
          ))}
          <LanguageMenu up />
        </nav>
      </div>
    </footer>
  );
}
