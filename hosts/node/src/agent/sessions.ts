/**
 * The agent's chats. A chief's chat is a run of sessions, each one
 * pi-durable conversation, one of them current. A peer's letters and its
 * questions each have a chat of their own (C22). Only these functions write
 * the `chat` table.
 */
import type { Context } from "@earendil-works/chord/context";
import type {
    ConversationCreateOptions,
    ConversationId,
    EntryDraft,
    Harness,
} from "@earendil-works/pi-durable";
import type { RawAccess } from "../actor/db/mod.ts";
import { withHarness } from "../pi/runtime.ts";
import { type AgentCall, generationOf } from "./actor.ts";
import type { ChatRef, Input, RequestId } from "./messages.ts";
import { onCopiedModel } from "./models.ts";
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
    if (input.type === "line" || input.type === "worker_answer") {
        return chiefSession(c);
    }
    return withHarness(c, generationOf(c).options, async (harness, context) => {
        const named = async (ref: string | undefined) => {
            const id = ref === undefined ? undefined : conversationOf(ref);
            return id === undefined
                ? undefined
                : harness.conversation(id, context);
        };
        switch (input.type) {
            case "letter":
            case "question": {
                const kind = input.type === "letter" ? "letters" : "questions";
                const peer = input.from.agent;
                return (
                    (await keptChat(c, kind, peer)) ??
                    made(c, [kind, peer, 0], () => created(c, harness, context))
                );
            }
            case "answer":
                return (
                    (await named(input.conversation)) ??
                    (await harness.root(context))
                ).id;
            // One the realm names no conversation for is the chief's.
            case "notice":
                return (await named(input.conversation))?.id ?? chiefSession(c);
        }
    });
}

export async function conversationOfChat(
    c: AgentCall,
    chat: ChatRef,
): Promise<ConversationId | undefined> {
    // A peer names a peer's chat, letters unless it says questions, and
    // `kind` alone names nothing more.
    if (chat.from !== undefined) {
        return Number.isSafeInteger(chat.from)
            ? keptChat(c, chat.kind ?? "letters", chat.from)
            : undefined;
    }
    return chat.past === undefined ? current(c.db) : session(c, chat.past);
}

/** The current session's conversation; with none, the root begins session 1. */
export function chiefSession(c: AgentCall): Promise<ConversationId> {
    return withHarness(
        c,
        generationOf(c).options,
        async (harness, context) =>
            (await current(c.db)) ??
            made(c, ["chief", 0, 1], () => harness.root(context), 1),
    );
}

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
    // A chat with no session begins session 1 first, so the first `/new`
    // answers 2.
    await chiefSession(c);
    const to = await withHarness(
        c,
        generationOf(c).options,
        async (harness, context) =>
            (
                await created(c, harness, context, (options) =>
                    harness.commit(async (tx) => {
                        const made = await tx.createConversation(options);
                        // As `crates/hamlet_server/src/talk/chats.rs` writes it.
                        await tx.appendEntry(
                            made.id,
                            noticeEntry("Started a new session."),
                        );
                        return made;
                    }, context),
                )
            ).id,
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

export async function makeCurrent(
    c: AgentCall,
    to: ConversationId,
): Promise<void> {
    await c.db.run(
        "UPDATE chat SET current = (conversation = ?) WHERE kind = 'chief'",
        to,
    );
}

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

/** The entry of a notice (C30), which the model never reads. */
export function noticeEntry(text: string): EntryDraft {
    return { kind: "hamlet.notice", data: { text, at: nowSeconds() } };
}

interface Begun {
    readonly number: number;
    readonly left: ConversationId | undefined;
}

async function begunBy(db: Rows, id: RequestId): Promise<Begun | undefined> {
    const row = await db.get<{
        session: number;
        followed: ConversationId | null;
    }>("SELECT session, followed FROM chat WHERE begun_by = ?", id);
    return row === undefined
        ? undefined
        : {
              number: row.session,
              left: row.followed ?? undefined,
          };
}

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

/**
 * A new conversation, empty unless `make` forks one, on the model the
 * agent's copy names. The copy is read once it exists, so `configured`
 * finds it for any copy kept later.
 */
export async function created(
    c: AgentCall,
    harness: Harness,
    context: Context,
    make: (options: ConversationCreateOptions) => Promise<Made> = (options) =>
        harness.createConversation(options, context),
): Promise<Made> {
    const conversation = await make({ ownership: { kind: "ownerless" } });
    await onCopiedModel(
        harness,
        context,
        conversation.id,
        generationOf(c).copies,
    );
    return conversation;
}

type Made = { readonly id: ConversationId };

/** A chat a concurrent input kept first wins, and its conversation is answered. */
async function made(
    c: AgentCall,
    key: readonly [kind: string, peer: number, session: number],
    make: () => Promise<Made>,
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
