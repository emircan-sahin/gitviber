import { failed, toast } from "./toast";

/** Copies `text` and says so: `what` titles the toast ("Path copied"), `detail` under it. */
export const copyText = (text: string, what: string, detail?: string) =>
  navigator.clipboard.writeText(text).then(() => toast("success", what, detail), failed("Could not copy"));
