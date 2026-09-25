import { createStore } from "../store";

/** Counts requests to show IdentityDialog: each one shows it again, even after "Not now". */
export const identityAsks = createStore(0);

/** From a commit git refused for want of a name or email. */
export const askForIdentity = () => identityAsks.set(identityAsks.get() + 1);
