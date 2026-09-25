// No DOM beyond `navigator`, which node:test has: commands.ts reads these under test.
const PLATFORM = typeof navigator === "undefined" ? "" : navigator.platform;
export const IS_MAC = /Mac|iPhone|iPad/.test(PLATFORM);
export const IS_WINDOWS = PLATFORM.startsWith("Win");

/** What the OS calls showing a file in its file manager. */
export const REVEAL_LABEL = IS_MAC ? "Reveal in Finder" : IS_WINDOWS ? "Show in Explorer" : "Show in Folder";
export const REVEAL_FAILED = `Could not ${REVEAL_LABEL[0].toLowerCase()}${REVEAL_LABEL.slice(1)}`;
