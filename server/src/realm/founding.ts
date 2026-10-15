import type { Context } from "@earendil-works/chord";
import type { DocumentReader, Tx } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import type { Deed, Part } from "../parts.ts";
/** The realm's founding and closing tools and prompt. */
export const Founding: Part = () => ({});
export const found: Deed = async () => {
    throw new Error("This is not built yet.");
};
export const hamlets: Deed = found;
export const nameHamlet: Deed = found;
export const moveHamlet: Deed = found;
export const removeHamletDeed: Deed = found;
export const restoreHamlet: Deed = found;
export const eraseHamlet: Deed = found;
/** The proposed founding and its origin. */
export type FoundingRequest = {
    hamlet: string;
    mayor: string;
    purpose: string;
    chiefs: string[];
    from: { hamlet: string; vote: number } | null;
};
/** A hamlet founded in one commit. */
export async function foundHamlet(
    _home: Home,
    _tx: Tx,
    _given: FoundingRequest,
): Promise<void> {
    return undefined;
}
/** The refusal of a proposed founding. */
export async function mayFound(
    _read: DocumentReader,
    _given: { hamlet: string; mayor: string; purpose: string },
    _context: Context,
): Promise<string | undefined> {
    return undefined;
}
/** A hamlet closed in one commit. */
export async function removeHamlet(
    _home: Home,
    _tx: Tx,
    _given: { hamlet: string; by: string; reason: string },
): Promise<void> {
    return undefined;
}
