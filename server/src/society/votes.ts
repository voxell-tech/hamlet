import type { Context } from "@earendil-works/chord";
import type { DocumentReader, Tx } from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import type { Deed, Part } from "../parts.ts";
import type { VoteWords } from "./docs.ts";
/** The hamlet's votes and office changes. */
export const Votes: Part = () => ({});
export const vote: Deed = async () => {
    throw new Error("This is not built yet.");
};
export const votes: Deed = vote;
export const overturn: Deed = vote;
export const mayor: Deed = vote;
/** The words holding decisions while a vote is open. */
export async function voteOpen(
    _read: DocumentReader,
    _hamlet: string,
    _now: number,
    _context: Context,
): Promise<string | undefined> {
    return undefined;
}
/** The latest votes for the window. */
export async function voteViews(
    _home: Home,
    _hamlet: string,
    _context: Context,
): Promise<VoteWords[]> {
    return [];
}
/** A change of the mayor's office. */
export async function seat(
    _home: Home,
    _tx: Tx,
    _given: { hamlet: string; number: number; by: string; reason: string },
): Promise<void> {
    return undefined;
}
