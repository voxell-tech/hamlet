import type { Deed, Part } from "../parts.ts";
/** The agent lifecycle and road tools. */
export const Agents: Part = () => ({});
/** A chief's road building deed. */
export const road: Deed = async () => {
    throw new Error("This is not built yet.");
};
/** A chief's road removal deed. */
export const unroad: Deed = road;
