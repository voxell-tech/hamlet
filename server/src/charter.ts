import { readFileSync } from "node:fs";
import type { Context } from "@earendil-works/chord";
import {
    type DocumentReader,
    defineDoc,
    type Session,
} from "@earendil-works/pi-durable";

/** The charter built into the server. */
export const DEFAULT_CHARTER = readFileSync(
    new URL("../assets/charter.md", import.meta.url),
    "utf8",
);

/** The home's own charter, `null` while it keeps the default (C12). */
export const CharterDoc = defineDoc<{ own: string | null }>({
    kind: "hamlet.charter",
    version: 1,
    scope: "session",
    initial: () => ({ own: null }),
});

/**
 * Gives the home `text` as its own charter. Nothing, a blank text or the default as it stands gives the default back,
 * so that the home keeps receiving later defaults (C12).
 */
export async function editCharter(
    session: Session,
    text: string | null,
    context: Context,
): Promise<void> {
    const own =
        text === null ||
        text.trim() === "" ||
        text.trim() === DEFAULT_CHARTER.trim()
            ? null
            : text;
    await session.commit(async (tx) => {
        (await tx.doc(CharterDoc)).own = own;
    }, context);
}

/** The charter every prompt opens with: the home's own, or the default. */
export async function charterOf(
    read: DocumentReader,
    context: Context,
): Promise<string> {
    return (await read.snapshot(CharterDoc, context))?.own ?? DEFAULT_CHARTER;
}
