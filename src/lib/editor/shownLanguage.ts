import { createStore } from "../store";

// The language of the code view on screen, for the status bar (which sits outside the viewer).
const shown = createStore<string | null>(null);
export const showLanguage = shown.set;
export const useShownLanguage = shown.use;
