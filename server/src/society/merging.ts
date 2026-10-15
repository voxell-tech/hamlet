import type { Context } from "@earendil-works/chord";
import type { Home } from "../home.ts";
import type { Deed, Part } from "../parts.ts";
import type { Archived } from "../record.ts";
/** Merges and their reversal. */
export const Merging: Part = () => ({});
export const merge: Deed = async () => {
    throw new Error("This is not built yet.");
};
/** A merged agent split back out of its kept house. */
export async function split(
    _home: Home,
    _given: {
        hamlet: string;
        entry: Archived;
        name: string;
        reason: string;
        chief: string;
    },
    _context: Context,
): Promise<string> {
    throw new Error("This is not built yet.");
}
