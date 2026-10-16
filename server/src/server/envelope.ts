import { type CommandFailure, commandFailureText } from "../commands.ts";
import {
    type ConnectionFailure,
    connectionFailureText,
} from "../connection.ts";
import { type WorkFailure, workFailureText } from "../environment.ts";
import { type NotesFailure, notesFailureText } from "../house/skills.ts";
import type { Fault, Malformed, Reply } from "../protocol.ts";
import {
    type ChiefRefusal,
    chiefRefusalText,
    type LegacyRefusal,
} from "../realm/access.ts";
import { type AddressFailure, addressFailureText } from "../realm/roads.ts";
import {
    type DecisionHeld,
    decisionText,
    type RecordRefusal,
    recordRefusalText,
} from "../record.ts";
import type { Result } from "../result.ts";
import type { Secrets } from "../scrub.ts";

/** A completed chief operation, before wire correlation and credential scrubbing. */
export type Receipt = {
    text: string;
    kind?: "notice" | "rewound";
    upload?: string;
};

/** Chief-facing failures whose wording is chosen at the protocol seam. */
export type ChiefFailure =
    | RecordRefusal
    | DecisionHeld
    | WorkFailure
    | NotesFailure
    | LegacyRefusal
    | AddressFailure
    | ChiefRefusal["error"]
    | CommandFailure
    | Malformed
    | ConnectionFailure;

/** Correlates and scrubs an operation's outcome exactly once for display. */
export function replyFor(
    to: string,
    outcome: Result<Receipt, ChiefFailure>,
    secrets: Secrets,
): Reply {
    return secrets.value({
        type: "reply",
        to,
        ...(outcome.ok
            ? { ...outcome.value, kind: outcome.value.kind ?? "notice" }
            : rendered(outcome.error)),
    });
}

/** A failed request or connection, without an invented request id. */
export function faultFor(
    to: string | undefined,
    failure: ChiefFailure,
    secrets: Secrets,
): Fault {
    return secrets.value({
        type: "fault",
        ...(to === undefined ? {} : { to }),
        text: rendered(failure).text,
    });
}

function rendered(failure: ChiefFailure): {
    text: string;
    kind: "notice" | "error";
} {
    switch (failure.kind) {
        case "vote-open":
            return { text: decisionText(failure), kind: "error" };
        case "agent-departed":
        case "name-format":
        case "name-reserved":
        case "name-taken":
        case "hamlet-full":
        case "not-mayor":
        case "vote-chief":
        case "vote-nonmember":
        case "vote-reason":
        case "vote-hamlet-missing":
        case "vote-overturn-chief":
        case "vote-mayor-chief":
        case "vote-already-open":
        case "vote-retry":
        case "vote-overturned":
        case "vote-not-passed":
        case "vote-mayor-changed":
        case "vote-too-few":
        case "vote-candidate":
        case "vote-missing":
        case "ballot-not-asked":
        case "ballot-choice":
        case "ballot-reason":
        case "ballot-ended":
        case "ballot-departed":
        case "ballot-closed":
        case "ballot-not-voter":
        case "ballot-already-cast":
        case "hamlet-title-long":
        case "hamlet-position":
        case "hamlet-purpose":
        case "hamlet-used":
        case "first-hamlet":
        case "house-move-pending":
        case "record-hamlet-missing":
        case "record-hamlet-chief":
        case "hamlets-full":
        case "hamlet-restore-name":
        case "founder-taken":
        case "agreement-mayor":
        case "realm-roads-closed":
        case "realm-road-office":
        case "road-agreed":
        case "move-pending":
        case "move-mayor":
        case "realm-road-local":
        case "realm-road-neither":
        case "move-already-there":
        case "move-neither":
        case "merge-too-large":
        case "merge-path":
        case "merge-source-unsafe":
        case "merge-path-taken":
        case "unsplit-merge":
        case "record-chief":
        case "merge-self":
        case "merged-purpose-empty":
        case "proposed-kept-missing":
        case "merge-changed":
        case "record-usage":
        case "erase-missing":
        case "model-pinned":
        case "model-not-allowed":
        case "proposed-mayor-missing":
        case "already-mayor":
        case "road-budget":
        case "mayor-untouchable":
        case "protected":
        case "purpose-empty":
        case "reason-empty":
        case "archive-missing":
        case "merge-unfinished":
        case "kept-departed":
        case "later-merge":
        case "road-exists":
        case "road-missing":
        case "roads-full":
        case "office-road":
        case "road-self":
            return { text: recordRefusalText(failure, "chief"), kind: "error" };
        case "folder-not-absolute":
        case "folder-not-directory":
        case "folder-overlaps-home":
            return { text: workFailureText(failure), kind: "error" };
        case "notes-not-utf8":
        case "notes-unreadable":
            return { text: notesFailureText(failure), kind: "error" };
        case "agent-missing":
            return {
                text:
                    "name" in failure
                        ? recordRefusalText(failure, "chief")
                        : addressFailureText(failure),
                kind: "error",
            };
        case "agent-gone":
            return { text: addressFailureText(failure), kind: "error" };
        case "hamlet-missing":
        case "hamlet-removed":
        case "not-chief":
            return {
                text: chiefRefusalText({ ok: false, error: failure }),
                kind: "error",
            };
        case "usage":
            return {
                text: commandFailureText(failure),
                kind: ["/to", "/mode", "/give"].includes(failure.command)
                    ? "error"
                    : "notice",
            };
        case "needs-number":
            return { text: commandFailureText(failure), kind: "notice" };
        case "legacy-refusal":
            return {
                text: connectionFailureText({
                    kind: "defect",
                    cause: failure.cause,
                }),
                kind: "error",
            };
        case "defect":
        case "connection":
            return { text: connectionFailureText(failure), kind: "error" };
        case "malformed":
            return { text: failure.text, kind: "error" };
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified chief failure: ${unhandled}`);
}
