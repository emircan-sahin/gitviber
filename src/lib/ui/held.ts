/**
 * A key's or a button's release goes to whatever has the keyboard or the pointer by then. Mission
 * Control (the swipe, F3, ⌃↑) takes it with no blur, as the window stays key; ⌘Tab and the screen
 * locking take it too. What the next event says about the keys and buttons is the truth.
 */

export type Modifier = "Meta" | "Control" | "Alt" | "Shift";

const FLAG = { Meta: "metaKey", Control: "ctrlKey", Alt: "altKey", Shift: "shiftKey" } as const;

// Wheel is a MouseEvent: a scroll with the hand on the trackpad tells too.
const INPUT = ["keydown", "keyup", "mousemove", "mousedown", "wheel"] as const;

/**
 * Runs `released` once, at the first sign `key` is up: an input event without it (its own keyup
 * among them), or the window going out of use (blur, hidden). Listens only while waiting; returns
 * a stop.
 */
export function untilReleased(key: Modifier, released: () => void): () => void {
  // Every key and mouse event carries the modifiers' state.
  const check = (e: Event) => !(e as MouseEvent | KeyboardEvent)[FLAG[key]] && done();
  const done = () => {
    stop();
    released();
  };
  const stop = () => {
    INPUT.forEach((t) => window.removeEventListener(t, check, { capture: true }));
    window.removeEventListener("blur", done);
    document.removeEventListener("visibilitychange", done);
  };
  INPUT.forEach((t) => window.addEventListener(t, check, { capture: true, passive: true }));
  window.addEventListener("blur", done);
  document.addEventListener("visibilitychange", done);
  return stop;
}

/**
 * For a drag's pointermove: whether its button went up out of the window's sight, which ends the
 * drag here instead of at the next click. Lets go of the pointer capture, which would otherwise
 * keep every hover elsewhere off until then.
 */
export function buttonLost(e: PointerEvent): boolean {
  if (e.buttons) return false;
  if (e.target instanceof Element && e.target.hasPointerCapture(e.pointerId)) e.target.releasePointerCapture(e.pointerId);
  return true;
}
