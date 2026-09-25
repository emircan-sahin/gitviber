import { type DependencyList, useEffect, useState } from "react";

/**
 * What `load` last resolved to, loaded again whenever `deps` change; `initial` until then. The
 * value stays while the next one loads, and a failed load keeps it (catch in `load` to replace
 * it). A reply that lands after unmount or after `deps` moved on is dropped. `load` null: none.
 */
export function useAsyncValue<T>(load: (() => Promise<T>) | null, deps: DependencyList, initial: T): T {
  const [value, setValue] = useState(initial);
  useEffect(() => {
    if (!load) return;
    let live = true;
    load().then(
      (v) => live && setValue(() => v),
      () => {},
    );
    return () => {
      live = false;
    };
  }, deps);
  return value;
}
