/**
 * A multi-line paste into the one-line Summary field (which WebKit would flatten into spaces),
 * split as git reads a message: its first line goes in at the caret (`from`–`to`), the rest
 * starts the description. Null for a one-line paste, which the field takes as usual.
 */
export function pasteMessage(draft: { summary: string; body: string }, template: string | null, text: string, from: number, to: number) {
  const [first, ...rest] = text.replace(/\r\n?/g, "\n").split("\n");
  if (!rest.length) return null;
  const more = rest.join("\n").replace(/^\s*\n/, "").trimEnd();
  const blank = !draft.body.trim() || draft.body === template;
  return {
    summary: draft.summary.slice(0, from) + first + draft.summary.slice(to),
    body: !more ? draft.body : blank ? more : `${more}\n\n${draft.body}`,
    caret: from + first.length,
  };
}
