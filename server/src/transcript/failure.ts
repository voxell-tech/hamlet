import { type ConversationFailure, conversationFailureText } from "../chats.ts";
import type { ChatRef } from "../protocol.ts";
import { type AddressFailure, addressFailureText } from "../realm/roads.ts";

/** A refused read or change to a chief's watched conversation. */
export type WatchFailure =
    | AddressFailure
    | ConversationFailure
    | ({ operation: string; chat: ChatRef; retrySafe: true } & (
          | { kind: "no-item"; before: string }
          | { kind: "read-only-peer" }
          | { kind: "past-read-only" }
          | {
                kind: "no-peer-conversation";
                reason: "choose-peer" | "session-number" | "missing";
            }
      ));

/** Chief-facing recovery for a watched conversation. */
export function watchFailureText(failure: WatchFailure): string {
    switch (failure.kind) {
        case "no-item":
            return `There is no item \`${failure.before}\` here.`;
        case "read-only-peer":
            return "Conversations between agents are read-only.";
        case "past-read-only":
            return "Open the current conversation to rewind it.";
        case "no-peer-conversation":
            switch (failure.reason) {
                case "choose-peer":
                    return "Choose the other agent's conversation.";
                case "session-number":
                    return "A peer conversation has no chief-session number.";
                case "missing":
                    return "There is no conversation with that agent here.";
            }
            break;
        case "busy":
        case "nothing-to-carry-on":
        case "no-past-session":
        case "chat-gone":
        case "not-chief-line":
        case "conversation-changed":
            return conversationFailureText(failure);
        case "agent-missing":
        case "agent-gone":
        case "hamlet-missing":
        case "hamlet-removed":
        case "not-chief":
            return addressFailureText(failure);
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified watch failure: ${unhandled}`);
}
