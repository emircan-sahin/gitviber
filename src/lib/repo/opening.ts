import { isInside } from "../path.ts";

/**
 * Runs calls one at a time, in the order asked, and skips one a later call overtook before it
 * started (its turn resolves to null). `latest`: no call was asked after this one.
 */
export function latestOnly() {
  let last = 0;
  let running = 0;
  let queue: Promise<unknown> = Promise.resolve();
  const run = <T>(fn: () => Promise<T>) => {
    const seq = ++last;
    const latest = () => seq === last;
    running++;
    const turn = queue.then(() => (latest() ? fn() : null));
    queue = turn.then(
      () => void running--,
      () => void running--,
    );
    return { turn, latest };
  };
  return { run, busy: () => running > 0 };
}

/** What to open in place of `gone`: its main worktree if that's another, else the project it was inside, else the first. */
export function fallbackFor(projects: string[], gone: string | null, main?: string) {
  const others = projects.filter((p) => p !== gone);
  return (main !== gone ? main : undefined) ?? others.find((p) => gone && isInside(gone, p)) ?? others[0];
}
