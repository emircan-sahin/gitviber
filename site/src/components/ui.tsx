import { Fragment, type ReactNode } from "react";
import { useCopy } from "../hooks/useCopy.ts";
import { useI18n } from "../i18n/index.tsx";
import { usePlatform } from "../hooks/usePlatform.ts";
import { release } from "../release.ts";
import { CheckIcon, CopyIcon, DownloadIcon } from "./icons.tsx";

export const cx = (...parts: (string | false | undefined)[]) => parts.filter(Boolean).join(" ");

export function CopyCommand({ command, className }: { command: string; className?: string }) {
  const [copied, copy] = useCopy();
  const { t } = useI18n();
  return (
    <div
      className={cx(
        "flex min-h-12 min-w-0 items-center gap-3 rounded-lg border border-border-strong bg-bg py-1.5 pr-1.5 pl-4 font-mono text-[13px]",
        className,
      )}
    >
      <span className="text-subtle select-none" aria-hidden>
        $
      </span>
      <code className="min-w-0 flex-1 text-left text-fg sm:overflow-x-auto sm:whitespace-nowrap sm:[scrollbar-width:none]">
        {/* Wraps between words on phones, never at the hyphens inside one. */}
        {command.split(" ").map((word, i) => (
          <Fragment key={i}>
            {i > 0 && " "}
            <span className="whitespace-nowrap">{word}</span>
          </Fragment>
        ))}
      </code>
      <button
        type="button"
        onClick={() => copy(command)}
        className="grid size-9 shrink-0 place-items-center rounded-md text-muted transition hover:bg-hover hover:text-fg"
        aria-label={copied ? t.cta.copied : t.cta.copy}
      >
        {copied ? <CheckIcon className="size-4 text-added" /> : <CopyIcon className="size-4" />}
      </button>
    </div>
  );
}

export function PrimaryLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a
      href={href}
      className="inline-flex h-12 shrink-0 items-center justify-center gap-2 rounded-lg bg-primary px-5 text-[15px] font-medium text-white transition hover:brightness-110 active:translate-y-px"
    >
      {children}
    </a>
  );
}

/** The one-click download for the visitor's OS; anything else goes to the install section. */
export function DownloadButton() {
  const platform = usePlatform();
  const { t } = useI18n();
  const target =
    platform === "mac"
      ? { href: release.assets.dmg, label: t.cta.downloadMac }
      : platform === "linux"
        ? { href: release.assets.appImage, label: t.cta.downloadLinux }
        : { href: "#install", label: t.cta.get };
  return (
    <PrimaryLink href={target.href ?? release.url}>
      <DownloadIcon className="size-[18px]" />
      {target.label}
    </PrimaryLink>
  );
}

export function Keys({ keys, className }: { keys: string[]; className?: string }) {
  return (
    <span className={cx("inline-flex items-center gap-1", className)}>
      {keys.map((k) => (
        <kbd key={k} className="kbd">
          {k}
        </kbd>
      ))}
    </span>
  );
}
