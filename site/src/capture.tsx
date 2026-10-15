// Dev-only page for scripts/capture/render.mjs: the story's window at an exact moment, so the
// README's animations are frames of the same UI the site plays. Not part of the site build.
import "@fontsource-variable/geist";
import "@fontsource-variable/jetbrains-mono";
import "./styles.css";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { AppWindow, WIN_H, WIN_W } from "./components/story/AppWindow.tsx";

const root = createRoot(document.getElementById("root")!);
Object.assign(window, {
  __size: { width: WIN_W, height: WIN_H },
  __frame(chapter: number, t: number) {
    flushSync(() => root.render(<div style={{ width: WIN_W, height: WIN_H }}><AppWindow chapter={chapter} t={t} /></div>));
  },
});
