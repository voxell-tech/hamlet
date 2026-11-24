/**
 * A client's following of the chats: a chat's page, the history of
 * sessions, and the `chat` events, numbered within the wake's epoch.
 */
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type {
    CommitChange,
    ConversationId,
    ConversationView,
    Cursor,
    EntryId,
    Harness,
} from "@earendil-works/pi-durable";
import { UserError } from "../actor/actor/mod.ts";
import { piRuntime, withHarness } from "../pi/runtime.ts";
import { type AgentCall, type AgentContext, generationOf } from "./actor.ts";
import { Fold, textOf, timeOf } from "./chat-items.ts";
import type {
    ChatEvent,
    ChatItem,
    ChatOp,
    ChatPage,
    ChatRef,
    ClientActions,
    Open,
    Session,
} from "./messages/agent.ts";
import { scrub, secretsLoaded } from "./models.ts";
import { chiefSession, conversationOfChat, current } from "./sessions.ts";

/** The number a client follows: one more than the last change, within `epoch`. */
export interface Following {
    readonly epoch: number;
    /** The number of the last event sent. */
    now(): number;
    send(chat: ChatRef, ops: readonly ChatOp[]): ChatEvent;
}

/** Adds one to `agent.epoch` and counts from 0. */
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

interface Watch {
    of?: Promise<unknown> | undefined;
    attached?: Promise<Attached>;
    current: ConversationId | undefined;
    /** One at a time, so the events go out in the order of the commits. */
    queue: Promise<void>;
    /** Conversations with changes folded and not sent, which their readers read again. */
    readonly unnamed: Set<ConversationId>;
}

interface Attached {
    readonly watch: Watch;
    readonly harness: Harness;
    readonly made: Set<ConversationId>;
    readonly folds: Map<ConversationId, Promise<Fold>>;
    /** Past sessions whose readers were told to read again, and none has. */
    readonly told: Set<ConversationId>;
}

const WATCHES = new WeakMap<Following, Watch>();

/** Turns each commit of the agent's conversations into `chat` events. */
export function watching(c: AgentContext, follow: Following): void {
    const watch: Watch = {
        current: undefined,
        queue: Promise.resolve(),
        unnamed: new Set(),
    };
    WATCHES.set(follow, watch);
    // `wake` begins to open the harness before it first waits.
    queueMicrotask(() => void attached(c, follow, watch)?.catch(warn(c)));
}

export function holdFollowers(c: AgentCall, answered: Promise<void>): void {
    const watch = WATCHES.get(generationOf(c).following);
    if (watch !== undefined) watch.queue = watch.queue.then(() => answered);
}

/** Attaches once per open, so a harness reopened after a failure is followed too. */
function attached(
    c: AgentContext,
    follow: Following,
    watch: Watch,
): Promise<Attached> | undefined {
    const runtime = piRuntime(c);
    const open = runtime.harness;
    if (runtime.status !== "open" || open === undefined) return undefined;
    if (watch.of !== open || watch.attached === undefined) {
        watch.of = open;
        watch.attached = open.then(({ harness }) =>
            attach(c, follow, watch, harness),
        );
        // A failed open is tried again by the next read.
        watch.attached.catch(() => {
            if (watch.of === open) watch.of = undefined;
        });
    }
    return watch.attached;
}

/**
 * A past session is folded once a client reads it; until then a change to
 * it only tells its readers to read again.
 */
async function attach(
    c: AgentContext,
    follow: Following,
    watch: Watch,
    harness: Harness,
): Promise<Attached> {
    const at: Attached = {
        watch,
        harness,
        made: new Set(),
        folds: new Map(),
        told: new Set(),
    };
    harness.observeCommits({
        advance: (publication) => {
            const touched = new Set<ConversationId>();
            for (const change of publication.changes) {
                // A conversation's record never changes after it is made.
                if (change.type === "conversation") {
                    const id = change.value.id;
                    at.made.add(id);
                    // An observer may not call the harness.
                    queueMicrotask(
                        () => void foldOf(c, follow, at, id).catch(warn(c)),
                    );
                    continue;
                }
                const id = conversationOfChange(change);
                if (id !== undefined && !at.folds.has(id)) touched.add(id);
            }
            for (const id of touched) changed(c, follow, at, id);
        },
        end: () => {},
    });
    harness.subscribeClose(() =>
        queueMicrotask(() => reopened(c, follow, watch)),
    );
    watch.current ??= await current(c.db);
    const rows = await c.db.all<{ conversation: ConversationId }>(
        "SELECT conversation FROM chat WHERE kind <> 'chief' OR current = 1",
    );
    for (const { conversation } of rows) {
        void foldOf(c, follow, at, conversation).catch(warn(c));
    }
    return at;
}

function conversationOfChange(
    change: Exclude<CommitChange, { type: "conversation" }>,
): ConversationId | undefined {
    switch (change.type) {
        case "entry":
        case "submission":
        case "task":
            return change.value.conversationId;
        default:
            return change.conversationId;
    }
}

/**
 * A past session's readers read again; a session made current since the
 * attach is folded from here on, and its readers read again too.
 */
function changed(
    c: AgentContext,
    follow: Following,
    at: Attached,
    conversation: ConversationId,
): void {
    const { watch } = at;
    watch.queue = watch.queue
        .then(async () => {
            // A conversation made in the same commit is folded whole.
            if (at.made.has(conversation) || at.folds.has(conversation)) {
                return;
            }
            const row = await rowOf(c, conversation);
            if (row === undefined) return;
            if (row.kind === "chief" && row.current === 0) {
                if (at.told.has(conversation)) return;
                at.told.add(conversation);
                follow.send(refOfRow(agentOf(c), row), [{ type: "reset" }]);
                return;
            }
            await foldOf(c, follow, at, conversation);
            if (row.kind === "chief") watch.current = conversation;
            follow.send(refOfRow(agentOf(c), row), [{ type: "reset" }]);
        })
        .catch(warn(c));
}

/**
 * A harness closed while the agent is open was reopened: the events of
 * the commits between are lost, so every chat is read again.
 */
function reopened(c: AgentContext, follow: Following, watch: Watch): void {
    watch.queue = watch.queue
        .then(async () => {
            if (piRuntime(c).status !== "open") return;
            const rows = await c.db.all<ChatRow>(
                "SELECT kind, peer, session, current FROM chat",
            );
            for (const row of rows) {
                follow.send(refOfRow(agentOf(c), row), [{ type: "reset" }]);
            }
            await attached(c, follow, watch);
        })
        .catch(warn(c));
}

/**
 * A conversation made after the attach is sent whole; any other starts from
 * its view when first folded, which a client reading in this epoch has.
 */
function foldOf(
    c: AgentContext,
    follow: Following,
    at: Attached,
    id: ConversationId,
): Promise<Fold> {
    const { watch } = at;
    const kept = at.folds.get(id);
    if (kept !== undefined) return kept;
    const made = (async () => {
        const fold = new Fold();
        const take = (view: ConversationView) => {
            watch.queue = watch.queue
                .then(async () => {
                    const loaded = await secretsLoaded(
                        generationOf(c).peers.reach,
                    ).then(
                        () => true,
                        (error: unknown) => {
                            warn(c)(error);
                            return false;
                        },
                    );
                    const ops = fold.view(view);
                    // Its readers read it again, which waits for the list.
                    if (!loaded) watch.unnamed.add(id);
                    await sent(c, follow, watch, id, loaded ? ops : []);
                })
                .catch(warn(c));
        };
        let early: ConversationView[] | undefined = [];
        const observation = await at.harness.observeConversation(
            id,
            {
                advance: (call) => {
                    if (call.kind !== "value") return;
                    if (early === undefined) take(call.value);
                    else early.push(call.value);
                },
                end: () => {},
            },
            BACKGROUND_CONTEXT,
        );
        const { before } = observation.value;
        if (before !== undefined) await counted(at.harness, id, before, fold);
        if (at.made.has(id)) take(observation.value);
        else fold.view(observation.value);
        for (const view of early) take(view);
        early = undefined;
        return fold;
    })();
    at.folds.set(id, made);
    // A failed attach is tried again by the next read.
    made.catch(() => at.folds.delete(id));
    return made;
}

/**
 * Folds into `fold` the entries of `id` up to `before`, which its view leaves
 * out, so it counts their answers and keeps their open calls.
 */
async function counted(
    harness: Harness,
    id: ConversationId,
    before: EntryId,
    fold: Fold,
): Promise<void> {
    const kept = await harness.conversation(id, BACKGROUND_CONTEXT);
    let cursor: Cursor | undefined;
    do {
        const page = await kept?.entries(
            { order: "ascending", maxEntryId: before },
            500,
            cursor,
            BACKGROUND_CONTEXT,
        );
        for (const entry of page?.items ?? []) fold.entry(entry);
        cursor = page?.next;
    } while (cursor !== undefined);
}

async function sent(
    c: AgentContext,
    follow: Following,
    watch: Watch,
    conversation: ConversationId,
    ops: readonly ChatOp[],
): Promise<void> {
    // An empty session a rewind moved the chief to shows no op, and its
    // readers still read again.
    const row = await rowOf(c, conversation);
    if (row === undefined) {
        if (ops.length > 0) watch.unnamed.add(conversation);
        return;
    }
    // Its chat was kept after a change was folded and not sent.
    let reset = watch.unnamed.delete(conversation);
    if (row.kind === "chief" && row.current === 1) {
        reset ||= watch.current !== undefined && watch.current !== conversation;
        watch.current = conversation;
    }
    if (reset) follow.send(refOfRow(agentOf(c), row), [{ type: "reset" }]);
    else if (ops.length > 0) follow.send(refOfRow(agentOf(c), row), ops);
}

type ChatRow = {
    readonly kind: "chief" | "letters" | "questions";
    readonly peer: number;
    readonly session: number;
    readonly current: number;
};

function rowOf(
    c: Pick<AgentContext, "db">,
    conversation: ConversationId,
): Promise<ChatRow | undefined> {
    return c.db.get<ChatRow>(
        "SELECT kind, peer, session, current FROM chat WHERE conversation = ?",
        conversation,
    );
}

function refOfRow(agent: number, row: ChatRow): ChatRef {
    if (row.kind !== "chief") {
        return { with: agent, from: row.peer, kind: row.kind };
    }
    return row.current === 1
        ? { with: agent }
        : { with: agent, past: row.session };
}

function agentOf(c: Pick<AgentContext, "address">): number {
    return Number(c.address.key);
}

function warn(c: Pick<AgentContext, "log">): (error: unknown) => void {
    return (error) => c.log.warn({ msg: "a chat was not followed", error });
}

/**
 * Called inside `withHarness`, which opens the harness even after a wake
 * that could not, so a page read is followed from its number on.
 */
async function followed(c: AgentCall, follow: Following): Promise<Attached> {
    const watch = WATCHES.get(follow);
    const at = watch === undefined ? undefined : attached(c, follow, watch);
    if (at === undefined) throw new Error("the agent's chats are not followed");
    return at;
}

/** A page holds at most this many items (`clients/requests.rs`). */
const PAGE = 100;

/**
 * Answers the page of `message.chat` ending before `message.before`, with
 * the number it was read at: every event numbered at most that is in it,
 * and a later one it already holds applies again to the same items.
 */
export async function readChat(c: AgentCall, message: Open): Promise<ChatPage> {
    const { chat, before } = message;
    const follow = generationOf(c).following;
    // As today's watch (`crates/hamlet_server/src/talk/chats.rs`
    // `current`), the chief's chat with no session begins session 1.
    const conversation =
        (await conversationOfChat(c, chat)) ??
        (chat.from === undefined && chat.past === undefined
            ? await chiefSession(c)
            : undefined);
    if (conversation === undefined) {
        // As `crates/hamlet_server/src/society/refusal.rs` words them.
        if (chat.from !== undefined) {
            throw refused(
                `no agent has the address \`${chat.from}'s ${chat.kind ?? "letters"} to ${chat.with}\``,
            );
        }
        throw refused(`there is no past session ${chat.past} here`);
    }
    const row = await rowOf(c, conversation);
    const { live, number, kept } = await withHarness(
        c,
        generationOf(c).options,
        async (harness, context) => {
            const at = await followed(c, follow);
            const live = await foldOf(c, follow, at, conversation);
            const number = follow.now();
            const fold = new Fold();
            const kept = new Map<string, ChatItem>();
            const held = await harness.conversation(conversation, context);
            let cursor: Cursor | undefined;
            do {
                const page = await held?.entries(
                    { order: "ascending" },
                    500,
                    cursor,
                    context,
                );
                for (const entry of page?.items ?? []) {
                    for (const op of fold.entry(entry)) {
                        if (op.type === "add") kept.set(op.item.id, op.item);
                    }
                }
                cursor = page?.next;
            } while (cursor !== undefined);
            return { live, number, kept };
        },
    );
    const items = [
        ...kept.values(),
        ...live.streamed().filter((item) => !kept.has(item.id)),
    ];
    const end =
        before === undefined
            ? items.length
            : items.findIndex((item) => item.id === before);
    // As `crates/hamlet_server/src/clients/requests.rs` words it.
    if (end === -1) throw refused(`There is no item \`${before}\` here.`);
    const start = Math.max(0, end - PAGE);
    const earlier = start > 0 ? items[start]?.id : undefined;
    return {
        epoch: follow.epoch,
        number,
        ...(row?.kind === "chief" ? { session: row.session } : {}),
        items: items.slice(start, end),
        ...(earlier === undefined ? {} : { earlier }),
        working: live.working,
        pending: live.pending,
        usage: live.usage,
    };
}

/** A refusal a client reads in `words`. */
export function refused(words: string): UserError {
    return new UserError(words, { code: "refused" });
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
                    modified = Math.max(
                        modified,
                        Math.floor(timeOf(entry) / 1000),
                    );
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
                tokens: "0",
                title: titleOf(scrub(first)),
            });
        }
        return { sessions };
    });
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
