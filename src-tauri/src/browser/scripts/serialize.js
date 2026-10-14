// How `eval` gives back its value (browser/agent.rs): `=` and JSON, but undefined, NaN,
// ±Infinity and a BigInt as themselves rather than as null (agent-browser #1936); inside an
// object or a list, as strings. The tail of the function body that has `value`.
const kept = (_key, v) => (typeof v === "bigint" ? `${v}n` : typeof v === "number" && !Number.isFinite(v) ? String(v) : v);
if (value === undefined) return "=undefined";
if (typeof value === "bigint" || typeof value === "number") return `=${kept("", value)}`;
try {
  const s = JSON.stringify(value, kept);
  return `=${s === undefined ? String(value) : s}`;
} catch {
  return `=${String(value)}`;
}
