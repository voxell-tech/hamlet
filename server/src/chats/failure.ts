import type { ChatId } from "./documents.ts";

/** A refused conversation deed, with the chat and operation needed for recovery. */
export type ConversationFailure = {
    operation: string;
    chat: ChatId;
    retrySafe: boolean;
} & (
    | { kind: "busy" }
    | { kind: "nothing-to-carry-on" }
    | { kind: "no-past-session"; number: number }
    | { kind: "chat-gone" }
    | { kind: "not-chief-line" }
    | { kind: "conversation-changed" }
);

/** Chief-facing wording shared by typed and protocol conversation deeds. */
export function conversationFailureText(failure: ConversationFailure): string {
    switch (failure.kind) {
        case "busy":
            return `A turn is running here. Stop it, then ${failure.operation}.`;
        case "nothing-to-carry-on":
            return "the last message was answered, so there is nothing to carry on";
        case "no-past-session":
            return `There is no past session \`${failure.number}\` here.`;
        case "chat-gone":
            return "The chat is gone.";
        case "not-chief-line":
            return "That is not a chief's message in this conversation.";
        case "conversation-changed":
            return "The current conversation changed. Try again.";
    }
    const unhandled: never = failure;
    throw new Error(`Unclassified conversation failure: ${unhandled}`);
}
