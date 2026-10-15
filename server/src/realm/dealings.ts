import type { Context } from "@earendil-works/chord";
import type { Home } from "../home.ts";
import type { Part } from "../parts.ts";
/** Agreements between hamlets. */
export const Dealings: Part = () => ({});
/** The hamlet's agreements still waiting on another mayor. */
export async function awaiting(
    _home: Home,
    _hamlet: string,
    _context: Context,
): Promise<string | undefined> {
    return undefined;
}
