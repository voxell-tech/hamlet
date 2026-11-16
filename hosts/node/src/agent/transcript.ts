/**
 * What a client reads and follows: entries as chat items, a chat's page,
 * the history of sessions, and the `chat` events, numbered within the
 * wake's epoch.
 */
import type { ConversationId, EntryRecord } from "@earendil-works/pi-durable";
import type { AgentCall, AgentContext } from "./actor.ts";
import type {
    ChatEvent,
    ChatItem,
    ChatOp,
    ChatPage,
    ChatRef,
    ClientActions,
    Open,
} from "./messages.ts";

/** The number a client follows: one more than the last change, within `epoch`. */
export interface Following {
    readonly epoch: number;
    /** The number of the last event sent. */
    now(): number;
    send(chat: ChatRef, ops: readonly ChatOp[]): ChatEvent;
}

/** Adds one to `agent.epoch` and counts from 0. Called once per wake. */
export async function following(
    c: Pick<AgentContext, "db" | "broadcast">,
): Promise<Following> {
    await c.db.run("UPDATE agent SET epoch = epoch + 1 WHERE id = 1");
    const row = await c.db.get<{ epoch: number }>(
        "SELECT epoch FROM agent WHERE id = 1",
    );
    const epoch = row?.epoch ?? 0;
    let number = 0;
    return {
        epoch,
        now: () => number,
        send: (chat, ops) => {
            number += 1;
            const event: ChatEvent = { chat, epoch, number, ops };
            c.broadcast("chat", event);
            return event;
        },
    };
}

/** Turns each commit of the agent's conversations into `chat` events. */
export function watching(c: AgentContext, follow: Following): void {
    void c;
    void follow;
}

/** One entry as the item a client shows, or `undefined` for one it does not show. */
export function itemOf(
    entry: EntryRecord,
    conversation: ConversationId,
): ChatItem | undefined {
    void entry;
    void conversation;
    throw new Error("not built: U3");
}

export function readChat(c: AgentCall, message: Open): Promise<ChatPage> {
    void c;
    void message;
    return Promise.reject(new Error("not built: U3"));
}

export function readHistory(
    c: AgentCall,
): Promise<ReturnType<ClientActions["read_history"]>> {
    void c;
    return Promise.reject(new Error("not built: U2"));
}
