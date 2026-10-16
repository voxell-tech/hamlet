import { type CommandFailure, commandFailureText } from "../commands.ts";
import {
    type ConnectionFailure,
    connectionFailureText,
} from "../connection.ts";
import type { Fault, Malformed, Reply } from "../protocol.ts";
import { type ChiefRefusal, chiefRefusalText } from "../realm/access.ts";
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
