import { createContext, useContext } from "react";

/** A settings row's label and hint ids: the Switch or Select beside them takes them as its name and description. */
export const FieldLabel = createContext<{ labelledBy?: string; describedBy?: string }>({});
export const useFieldLabel = () => useContext(FieldLabel);
