// Go to Definition in the code view, drawn by Monaco as in VS Code: ⌘-click or F12 goes where the
// name under it is defined, ⌥F12 peeks, ⇧F12 (or ⌘-click on a definition) lists where it's used,
// several places show as a list. definitions.rs finds names; an import path or path:line
// (lib/links) goes to the file it names. URLs are Monaco's own links.
import { api, DEFINITIONS_CANCELLED, type Definition, errorMessage, github } from "./api";
import { narrow, widenColumn } from "./indent";
import { commandIn } from "./keybindings";
import { languageFor } from "./language";
import { type LinkSide, type LinkTree, openTarget, resolverFor } from "./linkHost";
import { findLinks } from "./links";
import { createPeekModel, monaco, prepare, unitOf } from "./monaco";
import { getSettings } from "./settings";
import { toast } from "./toast";

/** What each model shows, read when asked: the worktree's revision moves on under it. */
const sides = new WeakMap<monaco.editor.ITextModel, () => LinkSide | null>();

/** Go to Definition and References in `editor`, for whatever it shows; `side`: where that is (null: nowhere to go). */
export function followDefinitions(editor: monaco.editor.ICodeEditor, side: () => LinkSide | null): monaco.IDisposable {
  const track = () => {
    const model = editor.getModel();
    if (model) sides.set(model, side);
  };
  track();
  const subs = [
    editor.onDidChangeModel(track),
    // Monaco's own F12 keys are off (lib/monaco): these follow the user's bindings.
    editor.onKeyDown((e) => {
      const command = commandIn(Object.keys(ACTIONS) as (keyof typeof ACTIONS)[], e.browserEvent);
      if (!command) return;
      e.preventDefault();
      e.stopPropagation();
      // Commands, not editor actions: getAction doesn't know them. They act on the focused editor.
      editor.trigger("keyboard", ACTIONS[command], null);
    }),
  ];
  return { dispose: () => subs.forEach((s) => s.dispose()) };
}

const ACTIONS = {
  "editor.goToDefinition": "editor.action.revealDefinition",
  "editor.peekDefinition": "editor.action.peekDefinition",
  "editor.goToReferences": "editor.action.goToReferences",
} as const;

monaco.languages.registerDefinitionProvider("*", {
  async provideDefinition(model, position) {
    const side = sides.get(model)?.();
    if (!side) return null;
    return (await linkAt(model, position, side)) ?? (await named("definitions", model, position, side));
  },
});

monaco.languages.registerReferenceProvider("*", {
  async provideReferences(model, position) {
    const side = sides.get(model)?.();
    return side ? named("references", model, position, side) : null;
  },
});

/** The file an import path or path:line at `pos` names. */
async function linkAt(model: monaco.editor.ITextModel, pos: monaco.Position, side: LinkSide): Promise<monaco.languages.LocationLink[] | null> {
  const lang = model.getLanguageId() === "plaintext" ? "text" : model.getLanguageId();
  // Ends included, as VS Code's links: the pointer's column is the caret spot nearest to it.
  const at = { start: pos.column - 1, end: pos.column - 1 };
  const link = findLinks(model.getLineContent(pos.lineNumber), lang, at).find((l) => pos.column >= l.start + 1 && pos.column <= l.end + 1);
  if (!link || link.kind === "url") return null;
  const target = (await resolverFor(side))(link);
  if (!target || "url" in target) return null;
  const file = await fileModel(target.path, side.tree);
  const column = (target.column ?? 1) - 1;
  return [
    {
      originSelectionRange: new monaco.Range(pos.lineNumber, link.start + 1, pos.lineNumber, link.end + 1),
      uri: file.uri,
      range: rangeIn(file, target.line ?? 1, column, column),
    },
  ];
}

/** Where the name at `pos` is defined, or used. */
async function named(kind: "definitions" | "references", model: monaco.editor.ITextModel, pos: monaco.Position, side: LinkSide): Promise<monaco.languages.Location[]> {
  const word = model.getWordAtPosition(pos);
  if (!word) return [];
  const unit = unitOf(model);
  const key = `${kind}:${model.uri}@${model.getVersionId()}:${pos.lineNumber}:${word.startColumn}:${treeKey(side.tree)}:${side.path}`;
  let found: Definition[];
  try {
    found = await cached(lookups, key, () =>
      api[kind]({
        path: side.path,
        text: narrow(model.getValue(), unit),
        line: pos.lineNumber,
        column: narrowColumn(model.getLineContent(pos.lineNumber), pos.column - 1, unit),
        rev: side.tree.rev,
      }),
    );
  } catch (e) {
    if (e !== DEFINITIONS_CANCELLED) console.warn(`Looking up ${kind} failed:`, errorMessage(e));
    return [];
  }
  const places = await Promise.all(
    found.map(async (d) => {
      const file = d.path === side.path ? model : await fileModel(d.path, side.tree);
      return { uri: file.uri, range: rangeIn(file, d.line, d.column, d.endColumn) };
    }),
  );
  trimFiles(new Set(places.map((p) => p.uri.toString())));
  return places;
}

// ⌘-hover asks as the pointer moves and the click asks again: each word once. A failed lookup
// (stopped by a newer one) isn't kept.
const lookups = new Map<string, Promise<Definition[]>>();
function cached<T>(map: Map<string, Promise<T>>, key: string, make: () => Promise<T>) {
  let value = map.get(key);
  if (value) return value;
  value = make();
  map.set(key, value);
  value.catch(() => map.get(key) === value && map.delete(key));
  if (map.size > 64) map.delete(map.keys().next().value!);
  return value;
}

/** A raw (unwidened) 0-based column, from a column in `line` as the view shows it. */
const narrowColumn = (line: string, column: number, unit: number) => narrow(line.slice(0, column), unit).length;

/** Raw columns on `line` of `model`, as a range in its widened text. */
function rangeIn(model: monaco.editor.ITextModel, line: number, column: number, endColumn: number) {
  const l = Math.min(Math.max(1, line), model.getLineCount());
  const unit = unitOf(model);
  const raw = narrow(model.getLineContent(l), unit);
  return new monaco.Range(l, widenColumn(raw, column, unit) + 1, l, widenColumn(raw, endColumn, unit) + 1);
}

// Other files, as models: Monaco's peek and its list read the places from them. Named by path and
// tree, so a file shows the version the definition was found in.
const SCHEME = "gitviber";
const treeKey = (tree: LinkTree) => tree.rev ?? `worktree@${tree.revision}`;
const files = new Map<string, Promise<monaco.editor.ITextModel>>();

function fileModel(path: string, tree: LinkTree) {
  const uri = monaco.Uri.from({ scheme: SCHEME, path: `/${path}`, query: treeKey(tree) });
  const key = uri.toString();
  let loading = files.get(key);
  if (!loading) files.set(key, (loading = load(path, tree, uri)));
  return loading;
}

async function load(path: string, tree: LinkTree, uri: monaco.Uri) {
  const f = await (tree.rev ? api.textAt(tree.rev, path) : api.readFile(path)).catch(() => null);
  // An image or a folder still opens in its tab; there's just no text to peek at.
  const text = f?.exists && !f.binary && !f.tooLarge ? f.text : "";
  const lang = languageFor(path, text);
  await prepare(lang, getSettings().codeTheme);
  const model = monaco.editor.getModel(uri) ?? createPeekModel(text, lang, uri);
  sides.set(model, () => ({ path, tree }));
  return model;
}

/** Disposes the oldest files past a few dozen, but none of `shown`: the list about to show them. */
function trimFiles(shown: Set<string>) {
  for (const [key, model] of files) {
    if (files.size <= 32) return;
    if (shown.has(key)) continue;
    files.delete(key);
    void model.then((m) => m.dispose(), () => {});
  }
}

/** For a repo switch: its worktree revisions start over, so its files' names would repeat. */
export function resetDefinitions() {
  lookups.clear();
  for (const model of files.values()) void model.then((m) => m.dispose(), () => {});
  files.clear();
}

// A place in another file opens that file's tab there.
monaco.editor.registerEditorOpener({
  openCodeEditor(_source, resource, selectionOrPosition) {
    if (resource.scheme !== SCHEME) return false;
    const pos = selectionOrPosition && ("startLineNumber" in selectionOrPosition ? { lineNumber: selectionOrPosition.startLineNumber, column: selectionOrPosition.startColumn } : selectionOrPosition);
    const model = monaco.editor.getModel(resource);
    const line = pos?.lineNumber ?? 1;
    const column = model && pos ? narrowColumn(model.getLineContent(line), pos.column - 1, unitOf(model)) + 1 : 1;
    openTarget({ path: resource.path.slice(1), line, column }, true);
    return true;
  },
});

// ⌘-click on a URL: the browser, not a window of the app.
monaco.editor.registerLinkOpener({
  open(uri) {
    if (uri.scheme !== "http" && uri.scheme !== "https") return false;
    github.openUrl(uri.toString(true)).catch((e) => toast("error", "Could not open the link", errorMessage(e)));
    return true;
  },
});
