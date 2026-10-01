import { createContext, useContext } from "react";

/** The id of a settings row's label: the Switch or Select beside it takes it as its name. */
export const FieldLabel = createContext<string | undefined>(undefined);
export const useFieldLabel = () => useContext(FieldLabel);
