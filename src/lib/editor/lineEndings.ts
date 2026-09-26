/**
 * Whether the file view can save `text` as it found it. Monaco turns every line ending into the
 * most common kind, so a file mixing CRLF and LF (or with a lone CR) would change on lines no one
 * touched. Pure, so it runs under `node --test`.
 */
export function keepsLineEndings(text: string) {
  const crlf = text.includes("\r\n");
  return !/\r(?!\n)/.test(text) && !(crlf && /(^|[^\r])\n/.test(text));
}
