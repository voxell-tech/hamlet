import { type ConversationFailure, conversationFailureText } from "../chats.ts";
import { type CommandFailure, commandFailureText } from "../commands.ts";
import {
    type ConnectionFailure,
    connectionFailureText,
} from "../connection.ts";
import type { Fault, Malformed, Reply } from "../protocol.ts";
import {
    type ChiefRefusal,
    chiefRefusalText,
    type LegacyRefusal,
} from "../realm/access.ts";
import { type AddressFailure, addressFailureText } from "../realm/roads.ts";
import type { Result } from "../result.ts";
import type { Secrets } from "../scrub.ts";
import { type WatchFailure, watchFailureText } from "../transcript.ts";

/** A completed chief operation, before wire correlation and credential scrubbing. */
export type Receipt = {
    text: string;
    kind?: "notice" | "rewound";
    upload?: string;
};

/** Chief-facing failures whose wording is chosen at the protocol seam. */
export type ChiefFailure =
    | WatchFailure
    | ConversationFailure
    | LegacyRefusal
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
