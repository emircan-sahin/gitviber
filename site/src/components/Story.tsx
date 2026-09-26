import {
  ArrowLeftRight,
  FolderGit2,
  FolderTree,
  GitBranchPlus,
  GitCommitHorizontal,
  ListChecks,
  SquareTerminal,
  Zap,
  type LucideIcon,
} from "lucide-react";
import { useEffect, useRef, useState, type ReactNode, type RefObject } from "react";
import { useInView } from "../hooks/useInView.ts";
import { useMediaQuery } from "../hooks/useMediaQuery.ts";
import { useReducedMotion } from "../hooks/useReducedMotion.ts";
import { chapters, CHAPTER_KEYS } from "../chapters.ts";
import { fill, rich, useI18n } from "../i18n/index.tsx";
import { BREW, release } from "../release.ts";
import { DiffRows, numbered } from "./app/Diff.tsx";
import { CommitBox, ExplorerPanel, FileHeader, GitPanel, Scaled, TerminalBody, TerminalTabs, Window, WorktreeMenu } from "./app/parts.tsx";
import { SCENES } from "./demo-data.ts";
import { AllCaughtUp, AppWindow, WIN_H, WIN_W, WORKTREES } from "./story/AppWindow.tsx";
import { AgentRun, CommitRows, KeyHud, ReviewRows, TerminalPanel } from "./story/panels.tsx";
import { clipLength, commitAt, explorerAt, HOLD_MS, landedAt, reviewAt, terminalAt, worktreesAt } from "./story/timeline.ts";
import { useElapsed } from "./story/useElapsed.ts";
import { usePlayhead } from "./story/usePlayhead.ts";
import { CopyCommand, cx, DownloadButton, Keys } from "./ui.tsx";

const ICONS: LucideIcon[] = [FolderGit2, Zap, ArrowLeftRight, GitBranchPlus, ListChecks, SquareTerminal, FolderTree, GitCommitHorizontal];

const windowShadow = "rounded-[10px] shadow-[0_50px_140px_-30px_rgb(0_0_0/0.95),0_0_0_1px_rgb(255_255_255/0.03)]";

// Before it docks, the window sits centered under the hero copy, larger and tilted back. The
// offsets are from its docked place in the right column (cqw: the layout grid's width).
const HERO_POSE = [
  "translate(calc((1 - var(--dock)) * (-17cqw - 16px)), calc((1 - var(--dock)) * (15svh + 21cqw)))",
  "rotateX(calc((1 - var(--dock)) * 18deg))",
  "scale(calc(1 + (1 - var(--dock)) * 0.3))",
].join(" ");

/** How far a clip has played, 0 to 1; a clip that isn't playing shows its end. */
const played = (chapter: number, t: number) => Math.min(1, t / clipLength(chapter));

/**
 * The page's story in one window, the way a product page keeps one device on screen: the hero's
 * window tilts up and docks in the right column, stays there, and plays each chapter while its
 * copy holds next to it. Phones stack the chapters, each with its own panel at real size.
 */
export function Story() {
  const { t } = useI18n();
  const ref = useRef<HTMLElement>(null);
  const dockRef = useRef<HTMLDivElement>(null);
  const reduced = useReducedMotion();
  const desktop = useMediaQuery("(min-width: 1024px)");
  const inView = useInView(ref);
  // -1 is the tour of the window, from the hero until the first chapter.
  const [chapter, setChapter] = useState(-1);
  const [elapsed, seek, wait] = usePlayhead(chapter, desktop && inView && !reduced, { length: clipLength(chapter), hold: HOLD_MS });

  // The window's dock progress, and each chapter's copy: held level with the window's top while
  // its clip plays, then fading out as the next one comes up.
  useEffect(() => {
    const el = ref.current;
    const dock = dockRef.current;
    if (!el || !dock) return;
    const blocks = [...el.querySelectorAll<HTMLElement>("[data-chapter]")];
    let lastY = scrollY;
    const update = () => {
      el.style.setProperty("--dock", Math.min(1, Math.max(0, -el.getBoundingClientRect().top / (innerHeight * 0.7))).toFixed(4));
      // The docked window's top, measured in its sticky box, which sits at the top of the screen.
      const pin = dock.offsetTop;
      el.style.setProperty("--pin", `${pin}px`);
      blocks.forEach((block, i) => {
        const copy = block.firstElementChild as HTMLElement;
        const r = block.getBoundingClientRect();
        const held = pin - r.top;
        const room = r.height - copy.offsetHeight;
        const fadeIn = held >= 0 ? 1 : Math.max(0.25, 1 + held / (innerHeight * 0.4));
        const fadeOut = i === blocks.length - 1 ? 1 : Math.min(1, Math.max(0, (room - held) / (room * 0.4)));
        block.style.setProperty("--fade", Math.min(fadeIn, fadeOut).toFixed(3));
      });
      // The last copy lets go when the window does, when the section's end reaches the screen's.
      const last = blocks[blocks.length - 1].firstElementChild as HTMLElement;
      el.style.setProperty("--tail", `${Math.max(0, innerHeight - pin - last.offsetHeight)}px`);
      const current = blocks.findLastIndex((b) => b.getBoundingClientRect().top <= innerHeight * 0.6);
      setChapter(Math.max(-1, current - 1));
      // A chapter's clip waits on its first frame until the copy settles next to it, so a short one
      // isn't over before it can be read. Then scrolling drives it too: by the time the copy starts
      // to fade, a fast scroll has played it through, and scrolling back rewinds it.
      const dy = scrollY - lastY;
      lastY = scrollY;
      const settled = current < 1 || pin - blocks[current].getBoundingClientRect().top >= 0;
      wait(!settled);
      if (current >= 0 && settled && dy) {
        const block = blocks[current];
        const room = block.offsetHeight - (block.firstElementChild as HTMLElement).offsetHeight;
        seek((dy * clipLength(current - 1)) / (room * 0.6));
      }
    };
    update();
    addEventListener("scroll", update, { passive: true });
    addEventListener("resize", update);
    return () => {
      removeEventListener("scroll", update);
      removeEventListener("resize", update);
    };
    // Another language changes the copy's heights.
  }, [seek, wait, t]);

  const copy = chapters(t);
  const slot = "lg:h-[140svh]";
  const held = "lg:sticky lg:top-[var(--pin)] lg:[opacity:var(--fade,1)]";

  return (
    <section ref={ref} id="top" className="relative [--dock:0]">
      <div aria-hidden data-nosnippet className="sticky top-0 hidden h-svh lg:block">
        <div className="mx-auto grid h-full max-w-[88rem] grid-cols-[minmax(0,0.34fr)_minmax(0,0.66fr)] items-center gap-12 px-8 pt-10 [container-type:inline-size]">
          <div ref={dockRef} className="col-start-2 [perspective:1800px]">
            <div className="origin-top" style={{ transform: HERO_POSE }}>
              <Scaled width={WIN_W} height={WIN_H} className={windowShadow}>
                <AppWindow chapter={chapter} t={elapsed} />
              </Scaled>
              <Progress count={copy.length + 1} current={chapter + 1} value={played(chapter, elapsed)} className="mt-5 [opacity:var(--dock)]" />
            </div>
          </div>
        </div>
      </div>

      <div className="relative lg:-mt-[100svh]">
        <Hero />
        <PhoneTour />

        <p className="sr-only">{t.agents.demo}</p>
        <div id="agents" className="mx-auto max-w-[88rem] px-5 pt-24 lg:grid lg:grid-cols-[minmax(0,0.34fr)_minmax(0,0.66fr)] lg:gap-12 lg:px-8 lg:pt-0">
          <div className="lg:pb-[var(--tail)]">
            {/* The window has just docked: say what it is while it shows itself part by part. */}
            <div data-chapter className={slot}>
              <div className={held}>
                <h2 className="text-4xl leading-[1.05] font-semibold tracking-[-0.03em] text-balance lg:text-5xl">{t.story.title}</h2>
                <p className="mt-5 max-w-md text-lg leading-relaxed text-pretty text-muted">{t.story.text}</p>
              </div>
            </div>
            {copy.map((c, i) => {
              const Icon = ICONS[i];
              return (
                <div key={i} data-chapter className={cx("py-20 lg:py-0", slot)}>
                  <div className={held}>
                    <p className="flex items-center gap-2 text-sm font-medium text-primary">
                      <Icon className="size-4" />
                      <span className="font-mono text-[12px] text-subtle">0{i + 1}</span>
                      {t.story.labels[i]}
                    </p>
                    <h2 className="mt-4 text-4xl leading-[1.05] font-semibold tracking-[-0.03em] text-balance lg:text-5xl">{c.title}</h2>
                    <p className="mt-5 max-w-md text-lg leading-relaxed text-pretty text-muted">
                      {rich(c.text, { keys: CHAPTER_KEYS[i] && <Keys keys={CHAPTER_KEYS[i]} className="align-middle" /> })}
                    </p>
                  </div>
                  <div aria-hidden data-nosnippet className="mt-12 lg:hidden">
                    <PhonePanel chapter={i} />
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
}

/** Where a clip is, like a video's scrubber: one segment per chapter, the playing one filling up. */
function Progress({ count, current, value, className }: { count: number; current: number; value: number; className?: string }) {
  return (
    <div aria-hidden className={cx("flex gap-1.5", className)}>
      {Array.from({ length: count }, (_, i) => (
        <span key={i} className="h-[3px] flex-1 overflow-hidden rounded-full bg-border-strong">
          <span
            className={cx("block h-full origin-left", i === current ? "bg-primary" : "bg-muted/50")}
            style={{ transform: `scaleX(${i < current ? 1 : i === current ? value : 0})` }}
          />
        </span>
      ))}
    </div>
  );
}

/** A clip on a phone: plays while on screen, starts over when it ends, with its own scrubber. */
function usePhoneClip(chapter: number, ref: RefObject<HTMLElement | null>) {
  const inView = useInView(ref, "0px 0px -25% 0px");
  const reduced = useReducedMotion();
  const t = useElapsed(chapter, inView && !reduced, { loop: clipLength(chapter) + HOLD_MS, idle: reduced ? Infinity : 0 });
  return { t, progress: <Progress count={1} current={0} value={played(chapter, t)} className="mx-auto mt-5 max-w-[322px]" /> };
}

/** The window under the hero on a phone, touring its parts. */
function PhoneTour() {
  const ref = useRef<HTMLDivElement>(null);
  const { t, progress } = usePhoneClip(-1, ref);
  return (
    <div ref={ref} aria-hidden data-nosnippet className="mx-auto max-w-xl px-5 lg:hidden">
      <Scaled width={WIN_W} height={WIN_H} className={windowShadow}>
        <AppWindow chapter={-1} t={t} />
      </Scaled>
      {progress}
    </div>
  );
}

function Hero() {
  const { t } = useI18n();
  return (
    <div className="flex flex-col items-center px-5 pt-[max(7rem,15vh)] pb-16 text-center lg:h-svh lg:pb-0 lg:[opacity:calc(1-var(--dock)*1.6)]">
      {release.version && (
        <a href={release.url} className="text-sm text-muted transition hover:text-fg">
          {fill(t.hero.release, { version: release.version })} <span aria-hidden>→</span>
        </a>
      )}
      <h1 className="mt-5 max-w-4xl text-[3.1rem] leading-[1.02] font-semibold tracking-[-0.035em] text-balance sm:text-7xl lg:text-[5.5rem]">
        {t.hero.title}
      </h1>
      <p className="mt-6 max-w-2xl text-lg leading-relaxed text-pretty text-muted sm:text-xl">{t.hero.lead}</p>
      <div className="mt-9 flex w-full flex-col items-center gap-3 sm:w-auto sm:flex-row">
        <DownloadButton />
        <CopyCommand command={BREW} className="w-full sm:w-auto" />
      </div>
    </div>
  );
}

/**
 * A chapter on a phone: just the part of the window it's about, at the app's real size, playing
 * while it's on screen.
 */
function PhonePanel({ chapter }: { chapter: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const { t, progress } = usePhoneClip(chapter, ref);

  let panel: ReactNode;
  if (chapter === 0) {
    const state = worktreesAt(t);
    panel = (
      <div className="flex justify-center">
        <WorktreeMenu rows={WORKTREES} current={1} highlighted={state.highlighted} />
      </div>
    );
  } else if (chapter <= 3) {
    panel = <PhoneScene index={chapter - 1} t={t} />;
  } else if (chapter === 4) {
    const state = reviewAt(t);
    panel = (
      <Frame>
        <GitPanel files={state.files} active={state.cursor} width={320} footer={false}>
          <ReviewRows state={state} />
        </GitPanel>
        <KeyHud label={state.key} className="right-3 bottom-3" />
      </Frame>
    );
  } else if (chapter === 5) {
    // One pane and no menu: a phone is narrower than a split.
    const state = terminalAt(t);
    panel = (
      <Window lights={false} className={cx("h-[250px]", windowShadow)}>
        <TerminalPanel state={{ ...state, panes: state.panes.slice(0, 1), menu: undefined }} wrap />
        <KeyHud label={state.key} className="right-3 bottom-3" />
      </Window>
    );
  } else if (chapter === 6) {
    // The tree on its own: quick open and the file view need a window's width.
    const state = explorerAt(t);
    panel = (
      <Frame>
        <div className="h-[332px]">
          <ExplorerPanel paths={state.tree} open={state.open} active={state.active} status={STATUS} width={320} />
        </div>
        <KeyHud label={state.key} className="right-3 bottom-3" />
      </Frame>
    );
  } else {
    const state = commitAt(t);
    panel = (
      <Frame>
        <GitPanel files={state.files} width={320} footer={<CommitBox {...state.box} />}>
          {/* Fixed height, so emptying the list doesn't move the page. */}
          <div className="h-[132px]">
            {state.files.length ? <CommitRows files={state.files} /> : <AllCaughtUp className="pt-5" />}
          </div>
        </GitPanel>
        <KeyHud label={state.key} className="right-3 bottom-3" />
      </Frame>
    );
  }
  return (
    <div ref={ref}>
      {panel}
      {progress}
    </div>
  );
}

const STATUS = Object.fromEntries(SCENES[0].files.map((f) => [f.path, f.status]));

function Frame({ children }: { children: ReactNode }) {
  return (
    <Window lights={false} className={cx("mx-auto max-w-[322px]", windowShadow)}>
      {children}
    </Window>
  );
}

/** An agent's worktree on a phone: the diff as it lands, and the agent's terminal under it. */
function PhoneScene({ index, t }: { index: number; t: number }) {
  const scene = SCENES[index];
  const lines = numbered(scene.lines, scene.start);
  const shown = landedAt(lines.length, t);
  const visible = lines.slice(0, shown);
  return (
    <Window lights={false} className={windowShadow}>
      <FileHeader
        path={scene.file}
        add={visible.filter((l) => l.kind === "+").length}
        del={visible.filter((l) => l.kind === "-").length}
        status={scene.files[0].status}
        compact
      />
      {/* Sized for the whole diff up front, so typing doesn't push the page down. */}
      <div className="overflow-hidden bg-bg" style={{ height: lines.length * 20 + 4 }}>
        <DiffRows lines={visible} fresh={shown < lines.length ? shown - 1 : undefined} />
      </div>
      <div className="border-t border-border">
        <TerminalTabs tabs={[{ folder: scene.folder, branch: scene.branch }]} />
        <TerminalBody wrap className="pb-3">
          <AgentRun scene={scene} typing={shown < lines.length} />
        </TerminalBody>
      </div>
    </Window>
  );
}
