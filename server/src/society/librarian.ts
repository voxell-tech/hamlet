import type { Context } from "@earendil-works/chord";
import type { Home } from "../home.ts";
import type { Deed, Part } from "../parts.ts";
import type { State } from "../protocol.ts";
/** The librarian's shelf and advice. */
export const Librarian: Part = () => ({});
export const unshelve: Deed = async () => {
    throw new Error("This is not built yet.");
};
/** The shared shelf for the window. */
export async function libraryView(
    _home: Home,
    _context: Context,
): Promise<NonNullable<State["library"]>> {
    return [];
}
