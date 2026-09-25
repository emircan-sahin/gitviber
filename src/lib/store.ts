import { useSyncExternalStore } from "react";

/**
 * A value that lives outside React (so any module can read or set it) and that components
 * re-render on: `use()` in a component, `subscribe` elsewhere. Setting the same value again
 * notifies no one.
 */
export function createStore<T>(initial: T) {
  let value = initial;
  const listeners = new Set<() => void>();
  const get = () => value;
  const subscribe = (l: () => void) => {
    listeners.add(l);
    return () => void listeners.delete(l);
  };
  return {
    get,
    set(next: T) {
      if (Object.is(next, value)) return;
      value = next;
      listeners.forEach((l) => l());
    },
    subscribe,
    use: () => useSyncExternalStore(subscribe, get),
  };
}
