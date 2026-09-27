/**
 * A multi-line paste into Summary, which WebKit would flatten into spaces, split as git reads a
 * message: its first line in at the caret (`from`–`to`), the rest starting the description, which
 * it replaces while that's empty or still `startBody`. Null for one line.
 */
export function pasteMessage(draft: { summary: string; body: string }, startBody: string, text: string, from: number, to: number) {
  const normalized = text.replace(/\r\n?/g, "\n");
  if (!normalized.includes("\n")) return null;
  const [first, ...rest] = normalized.replace(/^\s*\n/, "").split("\n");
  const more = rest.join("\n").replace(/^\s*\n/, "").trimEnd();
  const blank = !draft.body.trim() || draft.body === startBody;
  return {
    summary: draft.summary.slice(0, from) + first + draft.summary.slice(to),
    body: !more ? draft.body : blank ? more : `${more}\n\n${draft.body}`,
    caret: from + first.length,
  };
}
