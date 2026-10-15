import type { Context } from "@earendil-works/chord";
import type { Home } from "../home.ts";
import type { Deed, Part } from "../parts.ts";
/** Agents leaving and coming back. */
export const Leaving: Part = () => ({});
/** A chief's removal deed. */
export const remove: Deed = async () => {
    throw new Error("This is not built yet.");
};
export const restore: Deed = remove;
export const erase: Deed = remove;
export const record: Deed = remove;
export const undo: Deed = async () =>
    "Nothing to undo. Only a removal or a merge can be undone (the archive keeps the house); a permission is taken back with /revoke. A command that ran, a file that was changed and what was spent cannot be undone.";
/** The hamlet's history in words. */
export async function recordLines(
    _home: Home,
    _hamlet: string,
    _context: Context,
): Promise<string[]> {
    return [];
}
