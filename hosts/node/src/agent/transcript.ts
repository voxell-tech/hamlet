/**
 * What a client reads and follows: entries as chat items, a chat's page,
 * the history of sessions, and the `chat` events, numbered within the
 * wake's epoch.
 */
import { isDeepStrictEqual } from "node:util";
import { BACKGROUND_CONTEXT } from "@earendil-works/chord/context";
import type {
    AssistantMessage,
    ToolResultMessage,
} from "@earendil-works/pi-ai";
import {
    type CommitChange,
    type ConversationId,
    type ConversationView,
    type Cursor,
    type EntryRecord,
    type Harness,
    InboxDoc,
    type InboxState,
    LiveDoc,
    type LiveState,
    UsageDoc,
    type UsageState,
} from "@earendil-works/pi-durable";
import { UserError } from "../actor/actor/mod.ts";
import { piRuntime, withHarness } from "../pi/runtime.ts";
import { type AgentCall, type AgentContext, generationOf } from "./actor.ts";
import { clip, editOf } from "./change.ts";
import {
    type Author,
    type ChatEvent,
    type ChatItem,
    type ChatOp,
    type ChatPage,
    type ChatRef,
    type ClientActions,
    type ItemKind,
    type Open,
    type Session,
    type Usage,
    u32,
} from "./messages.ts";
import { conversationOfChat, current } from "./sessions.ts";
import { type Kept, NOTE_BREAK } from "./turn.ts";

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

/** What one wake's watches keep. */
interface Watch {
    /** The open of the agent's harness the watches follow. */
    of?: Promise<unknown> | undefined;
    attached?: Promise<Attached>;
    /** The conversation the last event of the chief's current chat showed. */
    current: ConversationId | undefined;
    /** One at a time, so the events go out in the order of the commits. */
    queue: Promise<void>;
    /** Conversations with changes folded while they had no chat. */
    readonly unnamed: Set<ConversationId>;
}

/** The watches of one open harness. */
interface Attached {
    readonly watch: Watch;
    readonly harness: Harness;
    /** The conversations made since it attached, which are sent whole. */
    readonly made: Set<ConversationId>;
    readonly folds: Map<ConversationId, Promise<Fold>>;
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

/**
 * Attaches the watches to the harness open now, once per open, so a
 * harness reopened after a failure is followed too.
 */
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
 * Folds the chief's current session and the peers' chats, and every
 * conversation made from now on. A past session is folded once a client
 * reads it; until then a change to it only tells its readers to read again.
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
 * A change to a conversation no fold follows: a past session's readers
 * read again, and a session made current since the attach is folded from
 * here on, its readers reading again too.
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
 * The fold of a conversation the watches follow. A conversation made
 * after the attach is sent whole; any other starts from its view when
 * first folded, which a client reading in this epoch already has.
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
                .then(() => sent(c, follow, watch, id, fold.view(view)))
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
        // The current chat is another session now.
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
 * The watches of the harness open now. Called inside `withHarness`, which
 * opens it even after a wake that could not, so a page read is followed
 * from its number on.
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
    const conversation = await conversationOfChat(c, chat);
    if (conversation === undefined) {
        // As `crates/hamlet_server/src/society/refusal.rs` words them.
        if (chat.from !== undefined) {
            throw refused(
                `no agent has the address \`${chat.from}'s ${chat.kind ?? "letters"} to ${chat.with}\``,
            );
        }
        if (chat.past !== undefined) {
            throw refused(`there is no past session ${chat.past} here`);
        }
        // The chief's chat before its first line, as today.
        await withHarness(c, generationOf(c).options, () =>
            followed(c, follow),
        );
        return {
            epoch: follow.epoch,
            number: follow.now(),
            items: [],
            working: false,
            pending: 0,
            usage: usageOf(undefined, 0),
        };
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

function refused(words: string): UserError {
    return new UserError(words, { code: "refused" });
}

/** An item and the content block it shows, with its call's id for a tool. */
interface Shown {
    readonly block: number;
    readonly item: ChatItem;
    readonly call?: string;
}

/**
 * Turns a conversation's entries and the answer being written into ops on
 * its items, as `crates/hamlet_server/src/clients/fold.rs` folds them. It
 * keeps only the answer being written and the calls not yet answered, so a
 * watch holds no text for the whole wake.
 */
class Fold {
    /** The item of each tool call by the call's id, until its result. */
    readonly #calls = new Map<string, ChatItem>();
    /** The items of the answer being written, by content block. */
    readonly #live = new Map<number, ChatItem>();
    #seen = -1;
    #lastInput = 0;
    working = false;
    pending = 0;
    usage: Usage = usageOf(undefined, 0);

    streamed(): ChatItem[] {
        return [...this.#live.values()];
    }

    /** Takes in a revision of the conversation's view: the ops that bring a client to it. */
    view(view: ConversationView): ChatOp[] {
        const ops: ChatOp[] = [];
        for (const entry of view.entries) {
            if (Number(entry.id) > this.#seen) ops.push(...this.entry(entry));
        }
        const live = view.docs[LiveDoc.definition.kind] as
            | LiveState
            | undefined;
        ops.push(
            ...this.#partial(
                live?.generation?.message as AssistantMessage | undefined,
            ),
        );
        const inbox = view.docs[InboxDoc.definition.kind] as
            | InboxState
            | undefined;
        const working = live?.run !== undefined;
        const pending =
            inbox?.items.filter((item) => item.mode !== "write").length ?? 0;
        if (working !== this.working || pending !== this.pending) {
            this.working = working;
            this.pending = pending;
            ops.push({ type: "status", working, pending });
        }
        const usage = usageOf(
            view.docs[UsageDoc.definition.kind] as UsageState | undefined,
            this.#lastInput,
        );
        if (!isDeepStrictEqual(usage, this.usage)) {
            this.usage = usage;
            ops.push({ type: "usage", usage });
        }
        return ops;
    }

    entry(entry: EntryRecord): ChatOp[] {
        this.#seen = Math.max(this.#seen, Number(entry.id));
        if (entry.kind === "pi.tool-result") return this.#ended(entry);
        const answer = entry.kind === "pi.assistant";
        const ops = itemsOf(entry).map(({ block, item, call }) => {
            if (call !== undefined) this.#calls.set(call, item);
            return added(item, answer ? this.#live.get(block)?.id : undefined);
        });
        if (answer) {
            this.#live.clear();
            const message = entry.model?.[0] as AssistantMessage | undefined;
            this.#lastInput = message?.usage.input ?? this.#lastInput;
        }
        return ops;
    }

    #ended(entry: EntryRecord): ChatOp[] {
        const result = entry.model?.[0] as ToolResultMessage | undefined;
        const item =
            result === undefined
                ? undefined
                : this.#calls.get(result.toolCallId);
        if (result === undefined || item?.tool === undefined) return [];
        this.#calls.delete(result.toolCallId);
        const text = result.content
            .map((part) => (part.type === "text" ? part.text : ""))
            .join("");
        return [
            added({
                ...item,
                tool: {
                    ...item.tool,
                    exit: result.isError ? 1 : 0,
                    ...(result.durationMs === undefined
                        ? {}
                        : { ms: u32(result.durationMs) }),
                    ...(result.isError ? { failure: failureOf(text) } : {}),
                },
            }),
        ];
    }

    #partial(message: AssistantMessage | undefined): ChatOp[] {
        if (message === undefined) return [];
        const ops: ChatOp[] = [];
        const at = Math.floor(message.timestamp / 1000);
        for (const { block, item } of blocksOf(
            message.content,
            (block) => `live:${block}`,
            at,
        )) {
            if (item.tool !== undefined) continue;
            const shown = this.#live.get(block);
            if (shown !== undefined && item.text === shown.text) continue;
            this.#live.set(block, item);
            ops.push(
                shown !== undefined && item.text.startsWith(shown.text)
                    ? {
                          type: "append",
                          item: item.id,
                          at: Buffer.byteLength(shown.text),
                          text: item.text.slice(shown.text.length),
                      }
                    : { type: "add", item },
            );
        }
        return ops;
    }
}

/** Adds `item`, or replaces the item with its id where it is. */
function added(item: ChatItem, replaces?: string): ChatOp {
    return {
        type: "add",
        item,
        ...(replaces === undefined ? {} : { replaces }),
    };
}

/** The words a chat shows where earlier entries were summarised (`clients/fold.rs`). */
const SUMMARY_NOTICE =
    "The conversation grew longer than the model can hold, so the earlier part of it was summarised.";

/** The items an entry shows; a tool's result shows in its call's item. */
function itemsOf(entry: EntryRecord): Shown[] {
    const at = timeOf(entry);
    const one = (
        kind: ItemKind,
        text: string,
        more: Partial<ChatItem> = {},
    ): Shown[] => [
        { block: 0, item: { id: `${entry.id}:0`, at, kind, text, ...more } },
    ];
    switch (entry.kind) {
        case "pi.assistant": {
            const message = entry.model?.[0] as AssistantMessage | undefined;
            if (message === undefined) return [];
            const id = (block: number) => `${entry.id}:${block}`;
            const shown = blocksOf(message.content, id, at);
            if (
                message.stopReason === "error" ||
                message.stopReason === "aborted"
            ) {
                const block = message.content.length;
                shown.push({
                    block,
                    item: {
                        id: id(block),
                        at,
                        kind: "error",
                        text: message.errorMessage ?? message.stopReason,
                    },
                });
            }
            return shown;
        }
        case "pi.compaction":
            return one("notice", SUMMARY_NOTICE);
        // A notice the realm told the model is an input; one for people
        // alone keeps its text in its data.
        case "hamlet.notice":
            return one(
                "notice",
                (entry.data as { text?: string } | undefined)?.text ??
                    textOf(entry),
            );
        // Today's words for a reflection's mark.
        case "hamlet.reflection":
            return one("notice", "reflection");
        case "hamlet.retry":
            return one("notice", textOf(entry));
        // The ask-again of a short cut-off (C28) shows as its summary's notice.
        case "hamlet.cut-off":
            return [];
        case "hamlet.line": {
            const line = one("user", textOf(entry), { rewindable: true });
            const { note } = (entry.data ?? {}) as Kept;
            return note === undefined
                ? line
                : [
                      ...line,
                      {
                          block: 1,
                          item: {
                              id: `${entry.id}:1`,
                              at,
                              kind: "notice",
                              text: note,
                          },
                      },
                  ];
        }
        case "hamlet.worker-answer": {
            const { from } = (entry.data ?? {}) as Kept;
            return one(
                "user",
                textOf(entry),
                from === undefined ? {} : { author: `worker#${from.agent}` },
            );
        }
        case "hamlet.letter":
        case "hamlet.question":
        case "hamlet.answer": {
            const { from } = (entry.data ?? {}) as Kept;
            return one(
                "user",
                textOf(entry),
                from === undefined ? {} : { author: authorOf(from) },
            );
        }
        default:
            return entry.kind === "pi.user" || entry.kind.startsWith("hamlet.")
                ? one("user", textOf(entry))
                : [];
    }
}

/** As `crates/hamlet_server/src/society/addresses.rs`'s `name_of` shows a hire. */
function authorOf(from: Author): string {
    const address = `hire#${from.agent}`;
    return from.name === undefined ? address : `${address} (${from.name})`;
}

function blocksOf(
    content: AssistantMessage["content"],
    id: (block: number) => string,
    at: number,
): Shown[] {
    const shown: Shown[] = [];
    for (const [block, part] of content.entries()) {
        const item = { id: id(block), at };
        if (part.type === "toolCall") {
            shown.push({
                block,
                call: part.id,
                item: {
                    ...item,
                    kind: "tool",
                    text: "",
                    tool: toolOf(part.name, part.arguments),
                },
            });
        } else if (part.type === "text" && part.text.trim() !== "") {
            shown.push({
                block,
                item: { ...item, kind: "assistant", text: part.text },
            });
        } else if (part.type === "thinking" && part.thinking.trim() !== "") {
            shown.push({
                block,
                item: { ...item, kind: "reasoning", text: part.thinking },
            });
        }
    }
    return shown;
}

/** Whole seconds: a notice's own time, else its message's. */
function timeOf(entry: EntryRecord): number {
    const message = entry.model?.[0];
    if (message !== undefined) return Math.floor(message.timestamp / 1000);
    const at = (entry.data as { at?: unknown } | undefined)?.at;
    return typeof at === "number" ? at : 0;
}

/** Characters of a tool's arguments an item shows (`clients/fold.rs`). */
const SUMMARY = 200;

function toolOf(name: string, args: unknown): NonNullable<ChatItem["tool"]> {
    const change = name === "edit" ? editOf(args) : undefined;
    return {
        name,
        // Today's server writes the arguments' keys sorted.
        summary: clip(
            JSON.stringify(args, (_key, value: unknown) =>
                value !== null &&
                typeof value === "object" &&
                !Array.isArray(value)
                    ? Object.fromEntries(
                          Object.entries(value).sort(([a], [b]) =>
                              a < b ? -1 : a > b ? 1 : 0,
                          ),
                      )
                    : value,
            ) ?? "null",
            SUMMARY,
        ),
        ...(change === undefined ? {} : { change }),
    };
}

/** The first line of a failed call's text that says something (`clients/fold.rs`). */
function failureOf(text: string): string {
    const line =
        text
            .replaceAll("<harness>\n", "")
            .replaceAll("</harness>\n", "")
            .replaceAll("<harness>", "")
            .replaceAll("</harness>", "")
            .split("\n")
            .map((part) => (part.startsWith("[error] ") ? part.slice(8) : part))
            .find((part) => part.trim() !== "") ?? "";
    return line.startsWith("Error: ") ? line.slice(7) : line;
}

/**
 * The conversation's spend, from pi-durable's ledger. It counts no calls
 * and no time, which today's server read from its own records.
 */
function usageOf(state: UsageState | undefined, lastInput: number): Usage {
    let input = 0;
    let cached = 0;
    let output = 0;
    let reasoning = 0;
    let dollars = 0;
    for (const used of Object.values(state?.models ?? {})) {
        input += used.input;
        cached += used.cacheRead;
        output += used.output;
        reasoning += used.reasoning ?? 0;
        dollars += used.cost.total;
    }
    const micros = Math.round(dollars * 1_000_000);
    return {
        calls: "0",
        unreported: "0",
        input: `${input}`,
        cached: `${cached}`,
        output: `${output}`,
        reasoning: `${reasoning}`,
        ms: "0",
        micros: `${micros}`,
        last_input: `${lastInput}`,
        summary: `${input} in, ${output} out, ${money(micros)}`,
    };
}

/** As `crates/hamlet_server/src/spend.rs` shows money. */
function money(micros: number): string {
    if (micros >= 1 && micros < 10_000) return "<$0.01";
    const cents = Math.floor((micros + 5_000) / 10_000);
    return `$${Math.floor(cents / 100)}.${`${cents % 100}`.padStart(2, "0")}`;
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

/** An entry's words; a reviewed line's are the chief's own, without the lead's note. */
export function textOf(entry: EntryRecord): string {
    const text = (entry.model ?? [])
        .flatMap((message) =>
            typeof message.content === "string"
                ? [message.content]
                : message.content.flatMap((part) =>
                      part.type === "text" ? [part.text] : [],
                  ),
        )
        .join("");
    const { note } = (entry.data ?? {}) as Kept;
    const noted = note === undefined ? undefined : `${NOTE_BREAK}${note}`;
    return noted !== undefined && text.endsWith(noted)
        ? text.slice(0, -noted.length)
        : text;
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
