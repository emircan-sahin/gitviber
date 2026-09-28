import type { Terminal } from "@xterm/xterm";

/**
 * Rows a wheel event scrolls, from the pixels still short of a row (`pending`) and its own: a row's
 * height of pixels is a row, the rest waits for the next event. Ghostty counts a trackpad this way
 * (Surface.zig scrollCallback), where xterm.js scales its pixels by 0.3.
 */
export function wheelRows(pending: number, pixels: number, rowHeight: number, maxRows = Infinity) {
  const total = pending + pixels;
  // A pane not laid out yet has no row height: counting by it would give Infinity or NaN rows.
  if (!(rowHeight > 0) || !Number.isFinite(total)) return { rows: 0, pending: Number.isFinite(pending) ? pending : 0 };
  const whole = Math.trunc(total / rowHeight);
  // Past `maxRows` the rows are dropped, not owed to the next event.
  const rows = Math.max(-maxRows, Math.min(maxRows, whole)) || 0;
  return { rows, pending: total - whole * rowHeight };
}

/**
 * In a program that reads the wheel (Claude Code's fullscreen view, vim, htop), a swipe reports a
 * row for each row it moves, as in Ghostty and cmux. xterm.js sends one report an event at most, so
 * a swipe moved Claude Code a few rows and a long one got nowhere. Each row is replayed to xterm as
 * a one-line wheel event, which it reports in whatever mouse encoding the program asked for.
 */
export function reportWheelByRow(term: Terminal) {
  let pending = 0;
  let replaying = false;
  term.attachCustomWheelEventHandler((e) => {
    // Shift is xterm's horizontal scroll; a line-mode wheel (and a replayed row) is left as it is.
    if (replaying || term.modes.mouseTrackingMode === "none" || e.shiftKey || e.deltaMode !== WheelEvent.DOM_DELTA_PIXEL || !term.element) return true;
    const screen = term.element.querySelector(".xterm-screen");
    if (!screen || !screen.clientHeight) return true;
    // A screenful at most: a flung wheel's huge delta would otherwise dispatch thousands of events at once.
    const step = wheelRows(pending, e.deltaY, screen.clientHeight / term.rows, term.rows);
    pending = step.pending;
    const row = { deltaY: Math.sign(step.rows), deltaMode: WheelEvent.DOM_DELTA_LINE, clientX: e.clientX, clientY: e.clientY, ctrlKey: e.ctrlKey, altKey: e.altKey, metaKey: e.metaKey, cancelable: true };
    replaying = true;
    try {
      for (let i = 0; i < Math.abs(step.rows); i++) term.element.dispatchEvent(new WheelEvent("wheel", row));
    } finally {
      replaying = false;
    }
    return false;
  });
}
