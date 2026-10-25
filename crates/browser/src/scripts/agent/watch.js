// What changes on the page from just before an action until its reply.

const DIALOGS = "dialog[open], [role=dialog], [role=alertdialog], [aria-modal=true]";

const dialogs = () => [...document.querySelectorAll(DIALOGS)].filter(shows).length;

/**
 * The page as an agent would find it again, the same after an action that did nothing: cheap
 * enough after any flood of changes, as it never reads the page's text.
 */
function pageState() {
  const focused = document.activeElement;
  const value = typeof focused?.value === "string" ? focused.value.slice(0, 200) : "";
  return [location.href, document.getElementsByTagName("*").length, Math.round(window.scrollY ?? 0), focused?.localName, value, dialogs()].join("|");
}

// A page's canvas changes through draw calls in the page's own world, out of this one's reach:
// a small picture of each shown canvas, before and after, tells whether it repainted. Each costs
// a draw and a readback of SAMPLE² px (well under a millisecond on a 2D canvas), three times an
// action at the most, plus AMBIENT ms of waiting when there is one.
const CANVASES = 4;
const SAMPLE = 48;
// How long a canvas gets to show it changes by itself, before the action: a few frames.
const AMBIENT = 50;

/** A hash of `canvas` drawn SAMPLE px square onto `scratch`; null when it can't be read (empty, or tainted by another origin's image). */
function sample(canvas, scratch) {
  try {
    scratch.width = scratch.height = SAMPLE;
    const ctx = scratch.getContext("2d", { willReadFrequently: true });
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(canvas, 0, 0, SAMPLE, SAMPLE);
    let hash = 2166136261;
    for (const byte of ctx.getImageData(0, 0, SAMPLE, SAMPLE).data) hash = Math.imul(hash ^ byte, 16777619);
    return hash >>> 0;
  } catch {
    return null;
  }
}

/**
 * The shown canvases of the page and its same-origin frames (CANVASES at most) as [canvas, hash,
 * animating]: animating when it painted otherwise within AMBIENT ms with nothing done to it.
 */
async function pictures() {
  const out = [];
  let scratch;
  for (const doc of documents()) {
    for (const canvas of doc.querySelectorAll?.("canvas") ?? []) {
      if (out.length === CANVASES) break;
      if (!shows(canvas)) continue;
      scratch ??= document.createElement("canvas");
      out.push([canvas, sample(canvas, scratch), false]);
    }
  }
  if (!out.length) return out;
  await new Promise((r) => setTimeout(r, AMBIENT));
  for (const picture of out) {
    const now = sample(picture[0], scratch);
    picture[2] = picture[1] !== null && now !== null && now !== picture[1];
    picture[1] = now;
  }
  return out;
}

/** Of `was` (pictures), the hash of the first still canvas that paints otherwise now; else "animating" if one was, else null. */
function repainted(was) {
  if (!was.length) return null;
  const scratch = document.createElement("canvas");
  let animating = false;
  for (const [canvas, hash, moving] of was) {
    if (hash === null || !canvas.isConnected) continue;
    if (moving) animating = true;
    else {
      const now = sample(canvas, scratch);
      if (now !== null && now !== hash) return now;
    }
  }
  return animating ? "animating" : null;
}

/** The page's document and each same-origin frame's, however deep. */
function documents(doc = document) {
  const frames = [...(doc.querySelectorAll?.("iframe") ?? [])].map(frameDoc).filter(Boolean);
  return [doc, ...frames.flatMap((inner) => documents(inner))];
}

/** What changes on the page from now, an action's, until `changes` reads it. */
async function watch() {
  state.watch?.observer.disconnect();
  const canvases = await pictures();
  const seen = { added: 0, removed: 0, texts: 0, attributes: 0, at: Date.now() };
  const count = (records) => {
    if (records.length) seen.at = Date.now();
    for (const r of records) {
      if (r.type === "attributes") seen.attributes++;
      else if (r.type === "characterData") seen.texts++;
      else {
        for (const n of r.addedNodes) {
          if (n.nodeType === 1) seen.added++;
          else if (n.nodeType === 3) seen.texts++;
        }
        for (const n of r.removedNodes) if (n.nodeType === 1) seen.removed++;
      }
    }
  };
  const observer = new MutationObserver(count);
  for (const doc of documents()) observer.observe(doc, { subtree: true, childList: true, characterData: true, attributes: true });
  state.watch = { observer, seen, count, dialogs: dialogs(), canvases };
  return {};
}

/**
 * What changed since `watch`, once the page has been still for `quiet` ms or `most` have gone
 * (a spinner never stops); the watch ends. None on another document: the watch went with the old.
 */
async function changes({ quiet = 100, most = 500 }) {
  const w = state.watch;
  if (!w) return { gone: true, state: pageState() };
  const until = Date.now() + most;
  while (Date.now() < until && Date.now() - w.seen.at < quiet) await new Promise((r) => setTimeout(r, 25));
  w.count(w.observer.takeRecords());
  w.observer.disconnect();
  state.watch = null;
  const { added, removed, texts, attributes } = w.seen;
  // A repaint is in the state too, so the loop nudge sees a canvas game move; an animation that
  // runs by itself is not, or no repeated action would ever be nudged.
  const canvas = repainted(w.canvases);
  return { added, removed, texts, attributes, canvas, dialogs: dialogs() - w.dialogs, state: pageState() + (typeof canvas === "number" ? `|canvas:${canvas}` : "") };
}
