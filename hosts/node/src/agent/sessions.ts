/**
 * The agent's chats. A chief's chat is a run of sessions, each one
 * pi-durable conversation, one of them current. A peer's letters and its
 * questions each have a chat of their own (C22). Only these functions write
 * the `chat` table.
 */
import type { ConversationId } from "@earendil-works/pi-durable";
import type { RawAccess } from "../actor/db/mod.ts";
import type { AgentCall } from "./actor.ts";
import type { ChatRef, Input, RequestId } from "./messages.ts";

export async function migrateChats(
    raw: Pick<RawAccess, "execute">,
): Promise<void> {
    await raw.execute(`
        CREATE TABLE IF NOT EXISTS chat (
            kind TEXT NOT NULL CHECK (kind IN ('chief', 'letters', 'questions')),
            peer INTEGER NOT NULL DEFAULT 0,
            session INTEGER NOT NULL DEFAULT 0,
            conversation INTEGER NOT NULL UNIQUE,
            begun_by TEXT UNIQUE,
            current INTEGER NOT NULL DEFAULT 0 CHECK (current IN (0, 1)),
            began INTEGER NOT NULL,
            PRIMARY KEY (kind, peer, session)
        ) STRICT
    `);
}

/** The conversation an input goes to, made when its chat has none yet. */
export function conversationFor(
    c: AgentCall,
    input: Input,
): Promise<ConversationId> {
    void c;
    void input;
    return Promise.reject(new Error("not built: U2"));
}

/** The conversation a client's `ChatRef` names, or `undefined`. */
export function conversationOfChat(
    c: AgentCall,
    chat: ChatRef,
): Promise<ConversationId | undefined> {
    void c;
    void chat;
    return Promise.reject(new Error("not built: U2"));
}

/**
 * Begins a session and makes it current. The request id is kept with it, so
 * a repeat of the same `/new` answers the same number. `left` is the
 * conversation of the session it leaves.
 */
export function begin(
    c: AgentCall,
    id: RequestId,
): Promise<{
    readonly number: number;
    readonly left: ConversationId | undefined;
}> {
    void c;
    void id;
    return Promise.reject(new Error("not built: U2"));
}

/** Makes session `number` current; `undefined` when there is none. */
export function resumed(
    c: AgentCall,
    number: number,
): Promise<
    | { readonly to: ConversationId; readonly left: ConversationId | undefined }
    | undefined
> {
    void c;
    void number;
    return Promise.reject(new Error("not built: U2"));
}

/** Points the current session at `fork`, the conversation a rewind made. */
export function moveTo(
    c: AgentCall,
    from: ConversationId,
    fork: ConversationId,
): Promise<void> {
    void c;
    void from;
    void fork;
    return Promise.reject(new Error("not built: U4"));
}
