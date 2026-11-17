/**
 * The agent's chats. A chief's chat is a run of sessions, each one
 * pi-durable conversation, one of them current. A peer's letters and its
 * questions each have a chat of their own (C22). Only these functions write
 * the `chat` table.
 */
import type { Conversation, ConversationId } from "@earendil-works/pi-durable";
import type { RawAccess } from "../actor/db/mod.ts";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, generationOf } from "./actor.ts";
import type { ChatRef, Input, RequestId } from "./messages.ts";
import { conversationOf } from "./turn.ts";

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
            followed INTEGER,
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
    return withHarness(c, generationOf(c).options, async (harness, context) => {
        switch (input.type) {
            case "line":
                return chiefNow(c, () => harness.root(context));
            case "letter":
            case "question": {
                const kind = input.type === "letter" ? "letters" : "questions";
                const peer = input.from.agent;
                return (
                    (await keptChat(c, kind, peer)) ??
                    made(c, [kind, peer, 0], () =>
                        harness.createConversation(
                            { ownership: { kind: "ownerless" } },
                            context,
                        ),
                    )
                );
            }
            case "answer": {
                const named = conversationOf(input.conversation);
                const asked =
                    named === undefined
                        ? undefined
                        : await harness.conversation(named, context);
                return (asked ?? (await harness.root(context))).id;
            }
        }
    });
}

/** The conversation a client's `ChatRef` names, or `undefined`. */
export async function conversationOfChat(
    c: AgentCall,
    chat: ChatRef,
): Promise<ConversationId | undefined> {
    // As today's server reads it: a peer names a peer's chat, letters
    // unless it says questions, and `kind` alone names nothing more.
    if (chat.from !== undefined) {
        return Number.isSafeInteger(chat.from)
            ? keptChat(c, chat.kind ?? "letters", chat.from)
            : undefined;
    }
    return chat.past === undefined ? current(c.db) : session(c, chat.past);
}

/** Session `number`'s conversation, or `undefined` when there is none. */
export async function session(
    c: AgentCall,
    number: number | bigint,
): Promise<ConversationId | undefined> {
    const asked = Number(number);
    if (!Number.isSafeInteger(asked)) return undefined;
    const row = await c.db.get<{ conversation: ConversationId }>(
        "SELECT conversation FROM chat WHERE kind = 'chief' AND session = ?",
        asked,
    );
    return row?.conversation;
}

/**
 * Begins a session and makes it current. The request id and the session it
 * left are kept with it, so a repeat of the same `/new` answers the same
 * number and can end the session left again.
 */
export async function begin(c: AgentCall, id: RequestId): Promise<Begun> {
    const before = await begunBy(c.db, id);
    if (before !== undefined) return before;
    const to = await withHarness(
        c,
        generationOf(c).options,
        async (harness, context) => {
            // As today, a chat with no session begins session 1 first, so
            // the first `/new` answers 2.
            await chiefNow(c, () => harness.root(context));
            return (
                await harness.createConversation(
                    { ownership: { kind: "ownerless" } },
                    context,
                )
            ).id;
        },
    );
    return c.db.transaction(async (tx) => {
        const raced = await begunBy(tx, id);
        if (raced !== undefined) return raced;
        const left = await current(tx);
        await tx.run("UPDATE chat SET current = 0 WHERE kind = 'chief'");
        await tx.run(
            `INSERT INTO chat (kind, session, conversation, begun_by, followed, current, began)
             SELECT 'chief', MAX(session) + 1, ?, ?, ?, 1, ? FROM chat WHERE kind = 'chief'`,
            to,
            id,
            left ?? null,
            nowSeconds(),
        );
        const begun = await begunBy(tx, id);
        if (begun === undefined)
            throw new Error("the new session was not kept");
        return begun;
    });
}

/** Makes the session of conversation `to` current. */
export async function makeCurrent(
    c: AgentCall,
    to: ConversationId,
): Promise<void> {
    await c.db.run(
        "UPDATE chat SET current = (conversation = ?) WHERE kind = 'chief'",
        to,
    );
}

/** Points the current session at `fork`, the conversation a rewind made. */
export async function moveTo(
    c: AgentCall,
    from: ConversationId,
    fork: ConversationId,
): Promise<void> {
    await c.db.run(
        "UPDATE chat SET conversation = ? WHERE kind = 'chief' AND conversation = ?",
        fork,
        from,
    );
}

type Rows = Pick<RawAccess, "get">;

/** A time the agent writes: whole seconds, which fit a `u32`. */
export function nowSeconds(): number {
    return Math.floor(Date.now() / 1000);
}

/** A session `/new` began, and the conversation of the one it left. */
interface Begun {
    readonly number: number;
    readonly to: ConversationId;
    readonly left: ConversationId | undefined;
}

async function begunBy(db: Rows, id: RequestId): Promise<Begun | undefined> {
    const row = await db.get<{
        session: number;
        conversation: ConversationId;
        followed: ConversationId | null;
    }>(
        "SELECT session, conversation, followed FROM chat WHERE begun_by = ?",
        id,
    );
    return row === undefined
        ? undefined
        : {
              number: row.session,
              to: row.conversation,
              left: row.followed ?? undefined,
          };
}

/** The current session's conversation. */
export async function current(db: Rows): Promise<ConversationId | undefined> {
    const row = await db.get<{ conversation: ConversationId }>(
        "SELECT conversation FROM chat WHERE kind = 'chief' AND current = 1",
    );
    return row?.conversation;
}

async function keptChat(
    c: AgentCall,
    kind: "letters" | "questions",
    peer: number,
): Promise<ConversationId | undefined> {
    const row = await c.db.get<{ conversation: ConversationId }>(
        "SELECT conversation FROM chat WHERE kind = ? AND peer = ?",
        kind,
        peer,
    );
    return row?.conversation;
}

/** The current session's conversation; the root begins session 1. */
async function chiefNow(
    c: AgentCall,
    root: () => Promise<Conversation>,
): Promise<ConversationId> {
    return (await current(c.db)) ?? made(c, ["chief", 0, 1], root, 1);
}

/**
 * Keeps the chat `key` with the conversation `make` gives. A chat a
 * concurrent input kept first wins, and its conversation is answered.
 */
async function made(
    c: AgentCall,
    key: readonly [kind: string, peer: number, session: number],
    make: () => Promise<{ readonly id: ConversationId }>,
    isCurrent: 0 | 1 = 0,
): Promise<ConversationId> {
    const { id } = await make();
    await c.db.run(
        "INSERT OR IGNORE INTO chat (kind, peer, session, conversation, current, began) VALUES (?, ?, ?, ?, ?, ?)",
        ...key,
        id,
        isCurrent,
        nowSeconds(),
    );
    const row = await c.db.get<{ conversation: ConversationId }>(
        "SELECT conversation FROM chat WHERE kind = ? AND peer = ? AND session = ?",
        ...key,
    );
    if (row === undefined) throw new Error(`the ${key[0]} chat was not kept`);
    return row.conversation;
}
