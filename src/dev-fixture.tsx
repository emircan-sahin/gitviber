// TEMP: renders the code view with a fixed diff so it can be inspected in Safari (same engine).
import { useState } from "react";
import { MonacoView } from "./features/MonacoView";
import type { DiffPair } from "./lib/api";

const text = Array.from({ length: 300 }, (_, i) => `line ${i + 1}: "value": ${i}`).join("\n") + "\n";
const pair: DiffPair = {
  original: { text: "", binary: false, tooLarge: false, exists: false, lossy: false },
  modified: { text, binary: false, tooLarge: false, exists: true, lossy: false },
  rows: Array.from({ length: 300 }, (_, i) => ({ k: 1 as const, o: 0, n: i + 1 })),
};

export function Fixture() {
  const [wrap, setWrap] = useState(new URLSearchParams(location.search).has("wrap"));
  return (
    <div className="flex h-full flex-col bg-background">
      <button className="h-8 text-left text-xs" onClick={() => setWrap(!wrap)}>
        wrap: {String(wrap)}
      </button>
      <div className="relative min-h-0 flex-1">
        <MonacoView pair={pair} path="a.json" mode="unified" collapse={false} wrap={wrap} scrollKey="fixture" />
      </div>
    </div>
  );
}
