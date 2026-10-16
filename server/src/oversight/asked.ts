import {
    defineDoc,
    defineDocFamily,
    type SubmissionId,
    type Tx,
} from "@earendil-works/pi-durable";
import type { With } from "../chats/conversations.ts";
import type { Danger } from "./waits.ts";

type Refused = { kind: Danger } | { exact: string };

/** The refusals of one conversation's current turn. */
export type Asked = {
    input: SubmissionId | null;
    strikes: number;
    questions: number;
    refused: { by: string; what: Refused; reason: string }[];
};

/** Per-turn asking state, reset when the input changes and kept through a restart. */
export const AskedDoc = defineDoc<Asked>({
    kind: "hamlet.asked",
    version: 2,
    migrate: (value) => ({ ...value, questions: 0 }) as Asked,
    scope: "conversation",
    history: "latest",
    fork: "initial",
    initial: () => ({ input: null, strikes: 0, questions: 0, refused: [] }),
});

/** The denial of a repeated or excess request in a turn (decide.rs:271-299). */
export function refused(
    asked: Readonly<Asked>,
    by: string,
    danger: Danger,
    about: string,
    cap: number,
): string | undefined {
    const what = refusal(danger, about);
    const earlier = asked.refused.find(
        (entry) =>
            entry.by === by &&
            ("kind" in what
                ? "kind" in entry.what && entry.what.kind === what.kind
                : "exact" in entry.what && entry.what.exact === what.exact),
    );
    if (earlier !== undefined)
        return `the chief already refused this in this turn (${earlier.reason}). Do not ask again: go on without it, or report that you could not.`;
    if (asked.strikes >= cap)
        return `the chief has refused ${asked.strikes} requests this turn and will not be asked again in it. Stop and report what you could not do.`;
    return undefined;
}

/** The refusal recorded for one denied approval. */
export function refuse(
    asked: Asked,
    by: string,
    danger: Danger,
    about: string,
    reason: string,
): void {
    asked.strikes += 1;
    asked.refused.push({ by, what: refusal(danger, about), reason });
}

/** The edits and commands trusted for one numbered conversation of a chat. */
export const TrustedDoc = defineDocFamily<
    { edit: boolean; command: boolean },
    string
>({
    kind: "hamlet.trusted",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ edit: false, command: false }),
});

/** The trust key, shared by forks that keep the chat's number. */
export function trustKey(place: With): string {
    return JSON.stringify([
        place.hamlet,
        place.number,
        place.chief,
        place.chat,
    ]);
}

/** The removal of a chat's trust when its conversation is resumed (H11). */
export async function resetTrust(tx: Tx, place: With): Promise<void> {
    const key = trustKey(place);
    Object.assign(
        await tx.doc(TrustedDoc, key, key),
        TrustedDoc.definition.initial(key),
    );
}

function refusal(danger: Danger, about: string): Refused {
    return danger === "network" || danger === "unconfined"
        ? { kind: danger }
        : { exact: about };
}
