/**
 * WebKit sends mousemove, and with it mouseleave, after a scroll or a layout change while the
 * pointer stands still. A list whose ↑↓ scrolls it then moved its highlight to whatever row slid
 * under the pointer, and a list that shrank under a filter dropped its highlight.
 */

let last: { x: number; y: number } | null = null;
// Bubbling to window runs after React's handlers, so they still see the position before this move.
window.addEventListener("mousemove", (e) => (last = { x: e.screenX, y: e.screenY }), { passive: true });

/** For a list's mousemove / mouseleave: whether the pointer really moved, not the page under it. */
export const pointerMoved = (e: { screenX: number; screenY: number }) => !last || last.x !== e.screenX || last.y !== e.screenY;
