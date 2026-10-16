import { type CommandFailure, commandFailureText } from "../commands.ts";
import {
    type ConnectionFailure,
    connectionFailureText,
} from "../connection.ts";
import { type AnswerFailure, answerFailureText } from "../gate.ts";
import { type ManifestFault, manifestFaultText } from "../hand/manifests.ts";
import type { Fault, Malformed, Reply } from "../protocol.ts";
import {
    type ChiefRefusal,
    chiefRefusalText,
    type LegacyRefusal,
} from "../realm/access.ts";
import { type AddressFailure, addressFailureText } from "../realm/roads.ts";
import type { Result } from "../result.ts";
import type { Secrets } from "../scrub.ts";
import { type ToolFailure, toolFailureText } from "../tools.ts";

/** A completed chief operation, before wire correlation and credential scrubbing. */
export type Receipt = {
    text: string;
    kind?: "notice" | "rewound";
    upload?: string;
};

/** Chief-facing failures whose wording is chosen at the protocol seam. */
export type ChiefFailure =
    | ManifestFault
    | ToolFailure
    | AnswerFailure
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
        case "not-json":
        case "missing-name":
        case "name-format":
        case "exec-empty":
        case "parameters-not-schema":
        case "schema-compile":
        case "limit-not-whole":
        case "name-reserved":
        case "name-taken":
        case "manifest-unreadable":
            return { text: manifestFaultText(failure), kind: "error" };
        case "not-a-folder":
            return {
                text:
                    failure.operation === "load-tool"
                        ? manifestFaultText(failure)
                        : toolFailureText(failure),
                kind: "error",
            };
        case "pattern-empty":
        case "pattern-invalid":
        case "pattern-not-linear":
        case "unreadable":
        case "no-environment":
        case "run-refused":
            return { text: toolFailureText(failure), kind: "error" };
        case "answer-not-offered":
            return { text: answerFailureText(failure), kind: "notice" };
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
