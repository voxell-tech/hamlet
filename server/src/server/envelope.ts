import { type CommandFailure, commandFailureText } from "../commands.ts";
import {
    type ConnectionFailure,
    connectionFailureText,
} from "../connection.ts";
import { type WorkFailure, workFailureText } from "../environment.ts";
import { type NotesFailure, notesFailureText } from "../house/skills.ts";
import {
    type GiftRefusal,
    giftRefusalText,
    type TreeFailure,
    treeFailureText,
} from "../house.ts";
import type { Fault, Malformed, Reply } from "../protocol.ts";
import {
    type ChiefRefusal,
    chiefRefusalText,
    type LegacyRefusal,
} from "../realm/access.ts";
import { type AddressFailure, addressFailureText } from "../realm/roads.ts";
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
    | TreeFailure
    | GiftRefusal
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
        case "entries-cap":
        case "bytes-cap":
        case "unsafe-entry":
        case "stage-not-empty":
        case "stage-overlaps-source":
        case "path-invalid":
        case "clobber":
        case "shelf-linked":
        case "refused":
            return { text: treeFailureText(failure), kind: "error" };
        case "skill-name":
        case "stage-refused":
        case "uploads-full":
        case "bytes-full":
        case "part-mismatch":
        case "upload-unknown":
        case "connection-closed":
        case "recipient-changed":
        case "gift-agent-missing":
        case "skill-exists":
        case "skill-missing":
        case "gift-manifest":
        case "skill-tree":
        case "gift-tree":
            return { text: giftRefusalText(failure), kind: "error" };

        case "folder-not-absolute":
        case "folder-not-directory":
        case "folder-overlaps-home":
            return { text: workFailureText(failure), kind: "error" };
        case "notes-not-utf8":
        case "notes-unreadable":
            return { text: notesFailureText(failure), kind: "error" };
        case "agent-missing":
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
