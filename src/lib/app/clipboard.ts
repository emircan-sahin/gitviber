import { api } from "../api";
import { failed, toast } from "./toast";

/** Copies `text` and says so: `what` titles the toast ("Path copied"), `detail` under it. */
export const copyText = (text: string, what: string, detail?: string) =>
  navigator.clipboard.writeText(text).then(() => toast("success", what, detail), failed("Could not copy"));

/** Copies files as Finder's ⌘C does: pasted into a terminal they become paths, elsewhere files. */
export const copyFiles = (paths: string[]) =>
  api.copyFiles(paths).then(() => toast("success", paths.length > 1 ? `${paths.length} files copied` : `${copyNoun(paths)} copied`), failed("Could not copy"));

/** The menu item that copies `paths`: "Copy Image" for one image, as VS Code's image preview names it. */
export const copyLabel = (paths: string[]) => (paths.length > 1 ? `Copy ${paths.length} Files` : `Copy ${copyNoun(paths)}`);

// The files clipboard.rs puts a picture on the pasteboard for.
const IMAGE = /\.(png|jpe?g|gif|webp|bmp|tiff?|heic|avif|ico)$/i;
const copyNoun = (paths: string[]) => (paths.length === 1 && IMAGE.test(paths[0]) ? "Image" : "File");
