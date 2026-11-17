/**
 * What a client reads and follows: entries as chat items, a chat's page,
 * the history of sessions, and the `chat` events, numbered within the
 * wake's epoch.
 */
import type {
    ConversationId,
    Cursor,
    EntryRecord,
} from "@earendil-works/pi-durable";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, type AgentContext, generationOf } from "./actor.ts";
import type {
    ChatEvent,
    ChatItem,
    ChatOp,
    ChatPage,
    ChatRef,
    ClientActions,
    Open,
    Session,
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

/**
 * The chief's sessions, newest first, each titled by its first line, as
 * `crates/hamlet_server/src/clients/documents.rs` lists them. A session
 * with no line yet is left out.
 */
export async function readHistory(
    c: AgentCall,
): Promise<ReturnType<ClientActions["read_history"]>> {
    const rows = await c.db.all<{
        session: number;
        conversation: ConversationId;
        began: number;
        current: number;
    }>(
        "SELECT session, conversation, began, current FROM chat WHERE kind = 'chief' ORDER BY session DESC",
    );
    return withHarness(c, generationOf(c).options, async (harness, context) => {
        const sessions: Session[] = [];
        for (const row of rows) {
            const conversation = await harness.conversation(
                row.conversation,
                context,
            );
            if (conversation === undefined) continue;
            let first: string | undefined;
            let messages = 0;
            let modified = row.began;
            let cursor: Cursor | undefined;
            do {
                const page = await conversation.entries(
                    { order: "ascending" },
                    500,
                    cursor,
                    context,
                );
                for (const entry of page.items) {
                    for (const message of entry.model ?? []) {
                        modified = Math.max(
                            modified,
                            Math.floor(message.timestamp / 1000),
                        );
                    }
                    if (entry.kind === "hamlet.line") {
                        messages += 1;
                        const text = textOf(entry);
                        if (first === undefined && text.trim() !== "") {
                            first = text;
                        }
                    } else if (entry.kind === "pi.assistant") {
                        messages += 1;
                    }
                }
                cursor = page.next;
            } while (cursor !== undefined);
            if (first === undefined) continue;
            sessions.push({
                number: row.session,
                began: row.began,
                modified,
                current: row.current === 1,
                messages,
                // Today's history counts no tokens either.
                tokens: "0",
                title: titleOf(first),
            });
        }
        return { sessions };
    });
}

export function textOf(entry: EntryRecord): string {
    return (entry.model ?? [])
        .flatMap((message) =>
            typeof message.content === "string"
                ? [message.content]
                : message.content.flatMap((part) =>
                      part.type === "text" ? [part.text] : [],
                  ),
        )
        .join("");
}

function titleOf(line: string): string {
    const words =
        line
            .split("\n")
            .find((part) => part.trim() !== "")
            ?.trim() ?? "";
    const chars = [...words];
    return chars.length > 90 ? `${chars.slice(0, 90).join("")}...` : words;
}
