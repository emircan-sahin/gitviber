// localStorage throws when it's full or turned off, and holds whatever an older build wrote:
// reads fall back, and nothing kept here is critical.

/** The JSON stored under `key`; `fallback` when there's none, it's null or unreadable, or `guard` rejects it. */
export function readJson<T>(key: string, fallback: T, guard?: (v: unknown) => v is T): T {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(key) ?? "null");
    if (guard) return guard(v) ? v : fallback;
    return (v as T | null) ?? fallback;
  } catch {
    return fallback;
  }
}

/** Stores `value` as JSON; false when it couldn't be (over quota, storage off). */
export function writeJson(key: string, value: unknown): boolean {
  try {
    localStorage.setItem(key, JSON.stringify(value));
    return true;
  } catch {
    return false;
  }
}

/** The strings in `v` when it's an array, else none: a saved list with anything else in it. */
export const stringList = (v: unknown): string[] => (Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : []);

export const isRecord = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);

/** Stores `value` as `id`'s in the object under `key` (null removes it), keeping the `max` most recently stored. */
export function putRecent(key: string, id: string, value: unknown, max: number): boolean {
  const { [id]: _, ...rest } = readJson(key, {}, isRecord);
  // Insertion order is recency: the one stored now goes last, the oldest drop off the front.
  const entries = [...Object.entries(rest), ...(value === null ? [] : [[id, value] as const])].slice(-max);
  return writeJson(key, Object.fromEntries(entries));
}
