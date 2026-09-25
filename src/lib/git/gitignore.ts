/**
 * A .gitignore line matching exactly this repo-relative path. The leading slash anchors it
 * (so `#` and `!` are never leading); glob characters and trailing spaces are escaped, or
 * `app/[id].tsx` would match `app/i.tsx` and not itself.
 */
export function ignorePattern(path: string): string {
  const escaped = path.replace(/[\\[\]*?]/g, "\\$&");
  const body = escaped.trimEnd();
  return `/${body}${"\\ ".repeat(escaped.length - body.length)}`;
}
