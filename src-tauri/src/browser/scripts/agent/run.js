// The command `input` names, run: its answer as JSON, or the error it gave.

// A page's lone half of a pair would reach the app as JSON it can't read.
function wellFormed(_key, value) {
  return typeof value === "string" ? value.toWellFormed() : value;
}

const run = { snapshot, find, get, aim, focus, focused, click, fill, type, press, select, check, hover, scroll, exists, hasText, view, watch, changes };

try {
  if (!Object.hasOwn(run, cmd)) throw new Error(`No ${cmd} here`);
  return JSON.stringify({ ...(await run[cmd](a)), last: state.next - 1 }, wellFormed);
} catch (e) {
  state.watch?.observer.disconnect();
  state.watch = null;
  return JSON.stringify({ error: String(e?.message ?? e), last: state.next - 1 }, wellFormed);
}
