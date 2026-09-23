import { api, CANCELLED, cancelNetwork, netOp, type RemoteTags } from "./api";

/** Reopening a menu within this reuses the last answer rather than asking the remote again. */
const FRESH_MS = 10_000;
/** A menu gives up on a remote that doesn't answer; git would wait minutes. */
const GIVE_UP_MS = 6_000;

let cached: { at: number; value: RemoteTags } | null = null;
let asking: Promise<RemoteTags> | null = null;
// Bumped by forgetRemoteTags: an answer asked for before a push must not be cached after it.
let generation = 0;

/**
 * The tags on the remote tags are pushed to, for the menus that show which are there. One
 * `git ls-remote` at a time, reused for a few seconds, and failing fast when offline.
 */
export function remoteTags(): Promise<RemoteTags> {
  if (cached && Date.now() - cached.at < FRESH_MS) return Promise.resolve(cached.value);
  if (!navigator.onLine) return Promise.reject(new Error("You're offline."));
  if (asking) return asking;
  const op = netOp();
  const gen = generation;
  const timer = setTimeout(() => void cancelNetwork(op), GIVE_UP_MS);
  const ask = api
    .remoteTags(op)
    .then(
      (value) => {
        if (gen === generation) cached = { at: Date.now(), value };
        return value;
      },
      (e) => {
        throw e === CANCELLED ? new Error("The remote didn't answer in time.") : e;
      },
    )
    .finally(() => {
      clearTimeout(timer);
      if (asking === ask) asking = null;
    });
  asking = ask;
  return ask;
}

/** Pushing or deleting a tag changes the answer. */
export function forgetRemoteTags() {
  cached = null;
  asking = null;
  generation++;
}
