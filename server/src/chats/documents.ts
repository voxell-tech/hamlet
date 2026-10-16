import {
    type ConversationId,
    defineDoc,
    defineDocFamily,
} from "@earendil-works/pi-durable";
import { type AgentRef, HOME } from "../record/document.ts";
/** The agent a conversation is with, the chief who talks to it, and its number in their chat (C22). */
export type With = {
    hamlet: string;
    /** The agent's number. */
    number: number;
    /** `null` for a conversation with no chief in it. */
    chief: string | null;
    chat: number;
};

/** Whom each conversation is with. */
export const WithDoc = defineDoc<With>({
    kind: "hamlet.with",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "current",
    initial: () => ({ hamlet: HOME, number: 0, chief: null, chat: 1 }),
});

/** A chat: an agent, and the chief who talks to it. */
export type ChatId = {
    agent: AgentRef;
    chief: string;
};

/** One conversation of a chat, by its number in the chat (C24). */
export type ChatConversation = {
    number: number;
    conversation: ConversationId;
    began: number;
};

/** A chief's chat with an agent: its conversations, and which one is in use (C22, C24). */
export type Chat = {
    current: number;
    conversations: ChatConversation[];
    /** Reflections submitted so far, which numbers the next one (C5). */
    reflections: number;
};

/** Each agent's chats, by `agentKey`, each by its chief. */
export const ChatDoc = defineDocFamily<
    { chats: { [chief: string]: Chat } },
    string
>({
    kind: "hamlet.chat",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ chats: {} }),
});

/** Every conversation belonging to an agent. */
export const AgentConversationsDoc = defineDocFamily<
    { ids: ConversationId[] },
    string
>({
    kind: "hamlet.agent-conversations",
    version: 1,
    scope: "session",
    family: true,
    initial: () => ({ ids: [] }),
});

/** The purpose of a one-off model call. */
export type ErrandKind = "ballot" | "answer" | "librarian" | "notes" | "review";

/** The role and prompt ownership of a one-off call. */
export const ErrandDoc = defineDoc<{
    kind: ErrandKind | null;
    own: boolean;
}>({
    kind: "hamlet.errand",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "current",
    initial: () => ({ kind: null, own: true }),
});
/** A conversation's paying hamlet (H14). */
export const PayerDoc = defineDoc<{ hamlet: string | null }>({
    kind: "hamlet.payer",
    version: 1,
    scope: "conversation",
    history: "latest",
    fork: "current",
    initial: () => ({ hamlet: null }),
});
