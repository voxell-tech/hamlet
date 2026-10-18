import { type ConversationFailure, conversationFailureText } from "../chats.ts";
import { type CommandFailure, commandFailureText } from "../commands.ts";
import {
    type ConnectionFailure,
    connectionFailureText,
} from "../connection.ts";
import { type WorkFailure, workFailureText } from "../environment.ts";
import { type AnswerFailure, answerFailureText } from "../gate.ts";
import { type NotesFailure, notesFailureText } from "../house/skills.ts";
import {
    type GiftRefusal,
    giftRefusalText,
    type MoveRefusal,
    moveRefusalText,
    type TreeFailure,
    treeFailureText,
} from "../house.ts";
import { type LibraryRefusal, libraryRefusalText } from "../library.ts";
import { type LimitsRefusal, limitsRefusalText } from "../limits.ts";
import {
    type CatalogueFailure,
    catalogueFailureText,
    type LoginFailure,
    loginFailureText,
    type ModelFailure,
    modelFailureText,
    providerFailureText,
    type SettingsFailure,
    settingsFailureText,
} from "../models.ts";
import type { Fault, Malformed, Reply } from "../protocol.ts";
import { type ChiefRefusal, chiefRefusalText } from "../realm/access.ts";
import { type AddressFailure, addressFailureText } from "../realm/roads.ts";
import {
    type DecisionHeld,
    decisionText,
    type RecordRefusal,
    recordRefusalText,
} from "../record.ts";
import type { Result } from "../result.ts";
import type { Secrets } from "../scrub.ts";
import { type WatchFailure, watchFailureText } from "../transcript.ts";
import {
    type WorkspaceUnavailable,
    workspaceUnavailableText,
} from "../workspace.ts";

/** A completed chief operation, before wire correlation and credential scrubbing. */
export type Receipt = {
    text: string;
    kind?: "notice" | "rewound";
    upload?: string;
};

/** Chief-facing failures whose wording is chosen at the protocol seam. */
export type ChiefFailure =
    | WorkspaceUnavailable
    | LibraryRefusal
    | RecordRefusal
    | DecisionHeld
    | AnswerFailure
    | LimitsRefusal
    | SettingsFailure
    | CatalogueFailure
    | LoginFailure
    | ModelFailure
    | TreeFailure
    | GiftRefusal
    | MoveRefusal
    | WorkFailure
    | NotesFailure
    | WatchFailure
    | ConversationFailure
    | AddressFailure
    | ChiefRefusal["error"]
    | CommandFailure
    | Malformed
    | ConnectionFailure;

/** Correlates and scrubs once; typed conversation refusals remain notices. */
export function replyFor(
    to: string,
    outcome: Result<Receipt, ChiefFailure>,
    secrets: Secrets,
    origin: "typed" | "protocol" = "protocol",
): Reply {
    return secrets.value({
        type: "reply",
        to,
        ...(outcome.ok
            ? { ...outcome.value, kind: outcome.value.kind ?? "notice" }
            : rendered(outcome.error, origin)),
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

function rendered(
    failure: ChiefFailure,
    origin: "typed" | "protocol" = "protocol",
): {
    text: string;
    kind: "notice" | "error";
} {
    switch (failure.kind) {
        case "workspace-unavailable":
            return { text: workspaceUnavailableText(failure), kind: "error" };
        case "library-stage":
        case "library-list":
        case "library-paper":
        case "library-name-taken":
        case "library-claimed":
        case "library-usage":
        case "library-skill-missing":
            return { text: libraryRefusalText(failure), kind: "error" };

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

        case "answer-not-offered":
            return { text: answerFailureText(failure), kind: "error" };
        case "invalid-field":
        case "unknown-field":
        case "votes-inconsistent":
            return { text: limitsRefusalText(failure), kind: "error" };

        case "reflect-every-not-whole":
        case "endpoint-name-invalid":
        case "endpoint-builtin":
        case "endpoint-duplicate":
        case "address-scheme":
        case "address-has-credentials":
        case "workspace-address":
        case "workspace-token":
            return { text: settingsFailureText(failure), kind: "error" };
        case "model-unknown":
        case "price-negative":
            return { text: catalogueFailureText(failure), kind: "error" };
        case "method-not-offered":
        case "port-in-use":
        case "login-unknown":
        case "login-not-waiting":
            return { text: loginFailureText(failure), kind: "error" };
        case "model-name-invalid":
        case "model-endpoint-missing":
        case "model-not-found":
            return { text: modelFailureText(failure), kind: "error" };
        case "provider-refused":
            return {
                text: providerFailureText(failure),
                kind: failure.operation === "sign-in" ? "error" : "notice",
            };

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
        case "skill-exists":
        case "skill-missing":
        case "gift-manifest":
        case "skill-tree":
        case "gift-tree":
            return { text: giftRefusalText(failure), kind: "error" };
        case "move-nothing":
        case "move-overridden":
        case "move-busy":
        case "move-stopped":
            return { text: moveRefusalText(failure), kind: "error" };

        case "folder-not-absolute":
        case "folder-not-remote":
        case "folder-not-directory":
        case "folder-overlaps-home":
            return { text: workFailureText(failure), kind: "error" };
        case "notes-missing":
        case "notes-not-utf8":
        case "notes-unreadable":
        case "notes-unavailable":
        case "notes-write-refused":
            return { text: notesFailureText(failure), kind: "error" };
        case "no-item":
        case "read-only-peer":
        case "past-read-only":
        case "no-peer-conversation":
            return { text: watchFailureText(failure), kind: "error" };
        case "busy":
        case "nothing-to-carry-on":
        case "no-past-session":
        case "chat-gone":
        case "not-chief-line":
        case "conversation-changed":
            return {
                text: conversationFailureText(failure),
                kind: origin === "typed" ? "notice" : "error",
            };
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
        case "defect":
        case "connection":
            return { text: connectionFailureText(failure), kind: "error" };
        case "malformed":
            return { text: failure.text, kind: "error" };
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified chief failure: ${unhandled}`);
}
