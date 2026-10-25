// A picked element's React components, nearest first (browser/macos/page_tools.rs). The body of an
// async function run in the page's own world, which alone sees React's fiber on the element;
// `nonce` is the tag picker.js left on it. A production build's names are minified.
const el = document.querySelector(`[data-gv-pick="${CSS.escape(nonce)}"]`);
if (!el) return "[]";
el.removeAttribute("data-gv-pick");
const key = Object.keys(el).find((k) => k.startsWith("__reactFiber$") || k.startsWith("__reactInternalInstance$"));
const names = [];
for (let fiber = key ? el[key] : null; fiber && names.length < 5; fiber = fiber.return) {
  const type = fiber.type;
  // Function and class components, and memo / forwardRef around one; not host elements (div).
  const inner = type && typeof type === "object" ? (type.type ?? type.render) : type;
  const name = typeof inner === "function" ? (type.displayName ?? inner.displayName ?? inner.name) : null;
  if (name && names.at(-1) !== name) names.push(name);
}
return JSON.stringify(names);
