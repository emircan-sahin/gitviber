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

/** The page's document and each same-origin frame's, however deep. */
function documents(doc = document) {
  const frames = [...(doc.querySelectorAll?.("iframe") ?? [])].map(frameDoc).filter(Boolean);
  return [doc, ...frames.flatMap((inner) => documents(inner))];
}

/** What changes on the page from now, an action's, until `changes` reads it. */
function watch() {
  state.watch?.observer.disconnect();
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
  state.watch = { observer, seen, count, dialogs: dialogs() };
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
  return { added, removed, texts, attributes, dialogs: dialogs() - w.dialogs, state: pageState() };
}
