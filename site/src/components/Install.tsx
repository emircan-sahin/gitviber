import { rich, useI18n } from "../i18n/index.tsx";
import { BREW, release, REPO_URL } from "../release.ts";
import { Logo } from "./icons.tsx";
import { CopyCommand, DownloadButton } from "./ui.tsx";

const LINUX = [
  { key: "deb", label: ".deb" },
  { key: "rpm", label: ".rpm" },
  { key: "appImage", label: "AppImage" },
] as const;

const link = "text-fg underline decoration-border-strong underline-offset-4 transition hover:decoration-fg";

export function Install() {
  const { t } = useI18n();
  const files = LINUX.map((f, i) => (
    <span key={f.key}>
      {i > 0 && ", "}
      <a className={link} href={release.assets[f.key] ?? release.url}>
        {f.label}
      </a>
    </span>
  ));
  return (
    <section id="install" data-section className="relative isolate overflow-hidden border-t border-border">
      <div
        aria-hidden
        className="absolute inset-x-0 bottom-0 -z-10 h-[80%] bg-[radial-gradient(50%_60%_at_50%_100%,rgb(74_159_245/0.12),transparent)]"
      />
      <div className="mx-auto flex max-w-3xl flex-col items-center px-5 py-32 text-center sm:px-8 sm:py-44">
        <Logo className="reveal size-20" />
        <h2 className="reveal mt-10 text-5xl leading-[1.02] font-semibold tracking-[-0.035em] text-balance [--delay:80ms] sm:text-7xl">
          {t.install.title}
        </h2>
        <p className="reveal mt-6 text-lg text-muted [--delay:140ms] sm:text-xl">{t.install.lead}</p>
        <div className="reveal mt-10 flex w-full flex-col items-center gap-3 [--delay:200ms] sm:flex-row sm:justify-center">
          <DownloadButton />
          <CopyCommand command={BREW} className="w-full sm:w-auto" />
        </div>
        <p className="reveal mt-8 text-[15px] leading-relaxed text-pretty text-muted [--delay:260ms]">
          {t.install.mac}
          <br />
          {rich(t.install.linux, { files })}
        </p>
        <p className="reveal mt-4 text-sm text-subtle [--delay:300ms]">
          {rich(t.install.source, {
            link: (
              <a className={link} href={`${REPO_URL}#build-from-source`}>
                {t.install.sourceLink}
              </a>
            ),
          })}
        </p>
      </div>
    </section>
  );
}
