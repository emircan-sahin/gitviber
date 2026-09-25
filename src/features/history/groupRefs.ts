import type { GraphRefs } from "@/lib/api";

/** `refs`: the full names the badge stands for (a local branch and its remote twin). */
type Ref = { name: string; kind: "head" | "local" | "remote" | "tag"; synced: boolean; refs: string[] };

/**
 * Groups decorations so they stay readable: a local branch and its remote twin on the same
 * commit (main + origin/main) become one "main ☁" badge; origin/HEAD is dropped, a detached
 * HEAD gets a badge of its own. `remotes` tells remote-tracking names apart, since local names
 * can contain "/" too. With `show`, only the refs it lets through, and always HEAD's branch.
 */
export function groupRefs(refs: string[], remotes: Set<string>, show?: GraphRefs): Ref[] {
  const head = refs.find((r) => r.startsWith("HEAD -> "))?.slice(8);
  const isRemote = (r: string) => remotes.has(r) || (!remotes.size && r.startsWith("origin/"));
  const short = (r: string) => r.slice(r.indexOf("/") + 1);
  const tag = (r: string) => r.startsWith("tag: ");
  const full = (r: string) => (tag(r) ? `refs/tags/${r.slice(5)}` : isRemote(r) ? `refs/remotes/${r}` : `refs/heads/${r}`);
  // Before grouping: with local branches off, main's remote twin still gets its badge.
  const shown = (r: string) =>
    !show || r === "HEAD" || r === head || ((tag(r) ? show.tags : isRemote(r) ? show.remote : show.local) && !show.hidden.includes(full(r)));
  const names = refs.map((r) => (r.startsWith("HEAD -> ") ? r.slice(8) : r)).filter(shown);
  const out: Ref[] = names.includes("HEAD") ? [{ name: "HEAD", kind: "head", synced: false, refs: [] }] : [];
  for (const r of names) {
    if (r === "HEAD" || r.endsWith("/HEAD")) continue;
    if (tag(r)) out.push({ name: r.slice(5), kind: "tag", synced: false, refs: [full(r)] });
    else if (isRemote(r)) {
      if (!names.includes(short(r))) out.push({ name: r, kind: "remote", synced: false, refs: [full(r)] });
    } else {
      const twins = names.filter((x) => isRemote(x) && short(x) === r);
      out.push({ name: r, kind: r === head ? "head" : "local", synced: twins.length > 0, refs: [full(r), ...twins.map(full)] });
    }
  }
  return out;
}
