// For `wait --fn` (browser/agent.rs): whether the expression `ask` gives is true yet. `+` it
// is; `-` and what it was; `!` and what it threw; `~` it didn't settle in `ms`, which counts as
// not yet. The tail of the function body that has `ask` and `ms`.
const pending = Symbol();
let value;
try {
  value = await Promise.race([ask(), new Promise((r) => setTimeout(r, ms, pending))]);
} catch (e) {
  return `!${e}`;
}
if (value === pending) return "~";
if (value) return "+";
try {
  return `-${JSON.stringify(value) ?? String(value)}`;
} catch {
  return `-${String(value)}`;
}
