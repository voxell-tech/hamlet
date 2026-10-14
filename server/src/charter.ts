import { readFileSync } from "node:fs";
import type { Context } from "@earendil-works/chord";
import {
    defineDoc,
    defineExtension,
    type Session,
    section,
} from "@earendil-works/pi-durable";

/** The charter built into the app. */
export const DEFAULT_CHARTER = readFileSync(
    new URL("../../assets/charter.md", import.meta.url),
    "utf8",
);

/** The home's own charter, `null` while it keeps the default (C12). */
export const CharterDoc = defineDoc<{ own: string | null }>({
    kind: "hamlet.charter",
    version: 1,
    scope: "session",
    initial: () => ({ own: null }),
});

/** Gives the home its own charter, or the default back for an empty one. */
export async function editCharter(
    session: Session,
    text: string,
    context: Context,
): Promise<void> {
    await session.commit(async (tx) => {
        (await tx.doc(CharterDoc)).own = text.trim() === "" ? null : text;
    }, context);
}

/** The charter as the first part of every prompt (C11). */
export const Charter = defineExtension({
    name: "hamlet.charter",
    sections: [
        section(
            "charter",
            async (input, context) =>
                (await input.read.snapshot(CharterDoc, context))?.own ??
                DEFAULT_CHARTER,
        ),
    ],
});
