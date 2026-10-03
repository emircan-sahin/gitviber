import type { Narrow, Scope } from "../../../lib/api/github.ts";
import { isRecord } from "../../../lib/storage.ts";

export type ListKind = "pulls" | "issues";

/** The chips, in the order they show: which accounts' work a list may be cut to (github/search.rs). */
export const SCOPES: { id: Scope; label: string; kinds: ListKind[]; phrase: string }[] = [
  { id: "created", label: "Created by me", kinds: ["pulls", "issues"], phrase: "created by you" },
  { id: "assigned", label: "Assigned to me", kinds: ["pulls", "issues"], phrase: "assigned to you" },
  { id: "mentioned", label: "Mentions me", kinds: ["pulls", "issues"], phrase: "that mention you" },
  { id: "reviewRequested", label: "Review requested", kinds: ["pulls"], phrase: "waiting for your review" },
];

export const scopesFor = (kind: ListKind) => SCOPES.filter((s) => s.kinds.includes(kind));

/** What the chips choose, kept for each repository; labels are picked apart and not kept. */
export type Choice = Pick<Narrow, "scope" | "draft">;

export const NO_CHOICE: Choice = { scope: null, draft: null };

/** One repository's saved choices, for pulls and for issues, as they were stored: anything unreadable is "All". */
export function parseChoice(saved: unknown, kind: ListKind): Choice {
  const v = isRecord(saved) ? saved[kind] : null;
  if (!isRecord(v)) return NO_CHOICE;
  const scope = SCOPES.find((s) => s.id === v.scope && s.kinds.includes(kind));
  return { scope: scope?.id ?? null, draft: kind === "pulls" && typeof v.draft === "boolean" ? v.draft : null };
}

/** `saved` with `choice` as `kind`'s; the other kind's stays. */
export const withChoice = (saved: unknown, kind: ListKind, choice: Choice) => ({ ...(isRecord(saved) ? saved : {}), [kind]: choice });

/** Whether anything narrows the list. */
export const isNarrowed = (n: Narrow) => n.scope !== null || n.draft !== null || n.labels.length > 0;

/** Tells one narrowing from another, for cache keys: "" for none, and the order labels were picked in doesn't matter. JSON: a label name may hold any separator. */
export const narrowKey = (n: Narrow) => (isNarrowed(n) ? JSON.stringify([n.scope, n.draft, [...n.labels].sort()]) : "");

/** What an empty list says: "No open pull requests assigned to you." */
export function emptyText(kind: ListKind, state: "open" | "closed" | "all", n: Narrow): string {
  const adjectives = [state === "all" ? "" : state, n.draft === null ? "" : n.draft ? "draft" : "non-draft"];
  const scope = SCOPES.find((s) => s.id === n.scope)?.phrase;
  const label = n.labels.length === 0 ? "" : n.labels.length === 1 ? "with this label" : "with all these labels";
  return `No ${[...adjectives, kind === "pulls" ? "pull requests" : "issues"].filter(Boolean).join(" ")}${[scope, label].filter(Boolean).map((t) => ` ${t}`).join("")}.`;
}
