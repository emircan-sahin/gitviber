// `data-window-hidden` on the root while the window is hidden (minimized, on another Space, or not
// shown yet at launch), so a working agent's pulse holds still (index.css).
const mark = () => document.documentElement.toggleAttribute("data-window-hidden", document.hidden);
mark();
document.addEventListener("visibilitychange", mark);
