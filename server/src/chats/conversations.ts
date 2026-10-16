import { randomUUID } from "node:crypto";
import type { Context } from "@earendil-works/chord";
import {
    type Conversation,
    type ConversationId,
    defineDoc,
    defineDocFamily,
    defineEntry,
    type InputSubmissionDraft,
    type Submission,
    type Tx,
} from "@earendil-works/pi-durable";
import type { Home } from "../home.ts";
import { configureAgent } from "../models/choice.ts";
import { chiefsOf, RealmDoc } from "../realm/places.ts";
import {
    type AgentRef,
    agentKey,
    HOME,
    RecordDoc,
} from "../record/document.ts";
import { InstructionsDoc } from "./instructions.ts";
import { notify } from "./notice.ts";
import { TaskDoc } from "./tasks.ts";
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

/** A line a chief typed, as its conversation keeps it (H24). */
export const ChiefEntry = defineEntry<{
    chief: string;
    text: string;
    /** The H12 task it starts. */
    task: string;
    at: number;
}>("hamlet.chief");

/** A chief's asking an agent to carry on a turn that ended unanswered (C26). */
export const RetryEntry = defineEntry<{ task: string; at: number }>(
    "hamlet.retry",
);

/** An agent's being asked to reflect (C5). */
export const ReflectionEntry = defineEntry<{ task: string; at: number }>(
    "hamlet.reflection",
);

/** A letter from another agent (H30). */
export const LetterEntry = defineEntry<{
    from: AgentRef;
    task: string;
    at: number;
    replyTo: ConversationId | null;
}>("hamlet.letter");
/** A question from an agent waiting for the answer (H30). */
export const QuestionEntry = defineEntry<{
    from: AgentRef;
    task: string;
    at: number;
}>("hamlet.question");
/** An answer returned to a letter's sender. */
export const ReplyEntry = defineEntry<{
    from: AgentRef;
    task: string;
    at: number;
}>("hamlet.reply");
/** An input to a one-off model call. */
export const ErrandEntry = defineEntry<{ task: string; at: number }>(
    "hamlet.errand",
);

/** The words a retry asks the model with (C26). */
const CARRY_ON = "Carry on from where you stopped.";

/** A chief's typed line, as an input that starts H12 task `task`. The line's `id` makes it admitted once. */
export function chiefInput(
    chief: string,
    line: { id: string; text: string },
    task: string,
    at: number,
): InputSubmissionDraft {
    return {
        type: "input",
        requestId: `chief:${line.id}`,
        content: line.text,
        entry: {
            kind: ChiefEntry.kind,
            data: { chief, text: line.text, task, at },
        },
    };
}

/** A retry, as an input of H12 task `task`. */
export function retryInput(task: string, at: number): InputSubmissionDraft {
    return {
        type: "input",
        requestId: `retry:${randomUUID()}`,
        content: CARRY_ON,
        entry: { kind: RetryEntry.kind, data: { task, at } },
    };
}

/** The reflection numbered `ordinal` of the conversation numbered `chat`, asked with `content`. */
export function reflectionInput(
    chat: number,
    ordinal: number,
    content: string,
    task: string,
    at: number,
): InputSubmissionDraft {
    return {
        type: "input",
        requestId: `reflect:${chat}:${ordinal}`,
        content,
        entry: { kind: ReflectionEntry.kind, data: { task, at } },
    };
}

/** Submits a chief's typed line to the current conversation of its chat, as a new task. */
export async function submitLine(
    home: Home,
    chat: ChatId,
    line: { id: string; text: string },
    context: Context,
): Promise<Submission> {
    const conversation = await chatOf(home, chat, context);
    const at = home.now();
    const task = `chief:${conversation.id}:${line.id}`;
    await home.harness.commit(async (tx) => {
        const key = agentKey(chat.agent);
        const doc = await tx.doc(InstructionsDoc, key, key);
        if (!doc.given.some((i) => i.task === task)) {
            doc.given.push({
                task,
                chief: chat.chief,
                text: line.text,
                at,
                done: null,
                hearing: { agent: false, mayor: false, cleared: false },
            });
            doc.given = doc.given.slice(-20);
        }
        (await tx.doc(TaskDoc, task, task)).chief = chat.chief;
    }, context);
    return conversation.submit(chiefInput(chat.chief, line, task, at), context);
}

/** The current conversation of `chat`, which its first use starts. */
export async function chatOf(
    home: Home,
    chat: ChatId,
    context: Context,
): Promise<Conversation> {
    const key = agentKey(chat.agent);
    const known = (await home.harness.snapshot(ChatDoc, key, context))?.chats[
        chat.chief
    ];
    const id =
        known === undefined
            ? await home.harness.commit(async (tx) => {
                  const chats = (await tx.doc(ChatDoc, key, key)).chats;
                  const existing = chats[chat.chief];
                  if (existing !== undefined) return currentOf(existing);
                  const conversation = await startConversation(home, tx, {
                      ...chat.agent,
                      chief: chat.chief,
                      chat: 1,
                  });
                  chats[chat.chief] = {
                      current: 1,
                      conversations: [
                          { number: 1, conversation, began: home.now() },
                      ],
                      reflections: 0,
                  };
                  return conversation;
              }, context)
            : currentOf(known);
    const conversation = await home.harness.conversation(id, context);
    if (conversation === undefined) {
        throw new Error(`The conversation ${id} of a chat is gone.`);
    }
    return conversation;
}

/** Starts a conversation with `place`, run with the agent's current choices. */
export async function startConversation(
    home: Home,
    tx: Tx,
    place: With,
): Promise<ConversationId> {
    const created = await tx.createConversation({
        ownership: { kind: "ownerless" },
    });
    Object.assign(await tx.doc(WithDoc, created.id), {
        ...place,
        chat: place.chief === null ? 0 : place.chat,
    });
    const key = agentKey(place);
    (await tx.doc(AgentConversationsDoc, key, key)).ids.push(created.id);
    await configureAgent(home, tx, created.id);
    return created.id;
}

/** The conversation `chat` uses now. */
export function currentOf(chat: Readonly<Chat>): ConversationId {
    const current = chat.conversations.find(
        (each) => each.number === chat.current,
    );
    if (current === undefined) {
        throw new Error(`A chat has no conversation ${chat.current}.`);
    }
    return current.conversation;
}

/** Every conversation of every chat an agent has, by the agent's chats. */
export function conversationsOf(chats: {
    readonly chats: { readonly [chief: string]: Readonly<Chat> };
}): ConversationId[] {
    return Object.values(chats.chats).flatMap((chat) =>
        chat.conversations.map((each) => each.conversation),
    );
}

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

/** Appends civic notices to every chief's chat with this hamlet's mayor. */
export async function tellChiefs(
    home: Home,
    tx: Tx,
    hamlet: string,
    text: string,
): Promise<void> {
    const r = await tx.doc(RecordDoc, hamlet, hamlet);
    const chiefs = chiefsOf(await tx.doc(RealmDoc), hamlet, home.chief);
    const key = agentKey({ hamlet, number: r.mayor });
    const chats = (await tx.doc(ChatDoc, key, key)).chats;
    for (const chief of chiefs) {
        let chat = chats[chief];
        if (!chat) {
            const conversation = await startConversation(home, tx, {
                hamlet,
                number: r.mayor,
                chief,
                chat: 1,
            });
            chats[chief] = {
                current: 1,
                conversations: [{ number: 1, conversation, began: home.now() }],
                reflections: 0,
            };
            chat = chats[chief];
        }
        if (chat) await notify(tx, currentOf(chat), text, home.now());
    }
}
