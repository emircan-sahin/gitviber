import { browserApi } from "../api";
import { ask } from "../app/ask";
import { failed, toast } from "../app/toast";

/** Settings → Browser and the palette: the browser tabs' cookies, storage and cache, asked first. */
export async function clearBrowsingData() {
  const ok = await ask("Clear the browser tabs' cookies, site storage and cache? Pages you're signed in to will ask you to sign in again. GitViber's own settings stay.", {
    title: "Clear browsing data",
    kind: "warning",
    okLabel: "Clear",
  });
  if (!ok) return;
  await browserApi.clearData().then(() => toast("success", "Browsing data cleared"), failed("Could not clear browsing data"));
}
