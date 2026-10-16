import type { Context } from "@earendil-works/chord";
import type { AssistantMessage, Message } from "@earendil-works/pi-ai";
import {
    AssistantEntry,
    CompactionEntry,
    type Conversation,
    type ConversationView,
    type Cursor,
    defineExtension,
    type EntryId,
    type EntryRecord,
    hook,
    type InboxState,
    type LiveState,
    ToolResultEntry,
    ToolTask,
    UserEntry,
} from "@earendil-works/pi-durable";
import { agentConversation } from "./agent-chats.ts";
import { SUMMARY_NOTICE } from "./answers.ts";
import { CHANGE, changeOf } from "./change.ts";
import {
    ChatDoc,
    ChiefEntry,
    chatOf,
    LetterEntry,
    QuestionEntry,
    ReflectionEntry,
    ReplyEntry,
    RetryEntry,
    WithDoc,
} from "./chats.ts";
import { usageOf } from "./history.ts";
import type { Home } from "./home.ts";
import { record } from "./models.ts";
import { NoticeEntry } from "./notice.ts";
import type {
    ChatRef,
    Item,
    LastActivity,
    Op,
    ServerMessage,
    Transcript,
    UsageView,
} from "./protocol.ts";
import { type ChiefResult, chiefAccess, chiefRefusalText } from "./realm.ts";
import { addressed, agentKey, RecordDoc } from "./record.ts";
import { Scrubber } from "./scrub.ts";

/** Characters of a tool's arguments a transcript shows (agent/event.rs:10). */
const SUMMARY = 200;

/** Entries read at once while a transcript is built. */
const PAGE = 500;

/** A chief's watch of one chat. */
export type ChatWatch = {
    stop(): Promise<void>;
};

/**
 * Watches `chat` for its chief: sends its transcript as it stands, with `to` when it answers a request, then a `turn`
 * for each step as it happens (C30, S5). Applied in order, the steps give the transcript read again.
 */
export async function watchChat(
    home: Home,
    chat: ChatRef,
    send: (message: ServerMessage) => void,
    to: string | undefined,
    context: Context,
    before?: string,
): Promise<ChiefResult<ChatWatch>> {
    const access = await chiefAccess(
        home,
        chat.with.hamlet,
        "watch chat",
        context,
    );
    if (!access.ok) return access;
    let stopped = false;
    let watch: Awaited<ReturnType<Conversation["watch"]>> | undefined;
    let id: Conversation["id"] | undefined;
    const bind = async () => {
        const outcome = await conversationOf(home, chat, context);
        if (!outcome.ok) {
            await watch?.stop();
            send({
                type: "fault",
                ...(to === undefined ? {} : { to }),
                text: chiefRefusalText(outcome),
            });
            return;
        }
        const conversation = outcome.value;
        if (id === conversation.id || stopped) return;
        await watch?.stop();
        if (stopped) return;
        const rebound = id !== undefined;
        id = conversation.id;
        watch = await conversation.watch(context);
        if (stopped) {
            await watch.stop();
            return;
        }
        const number = chat.from
            ? undefined
            : (await home.harness.snapshot(WithDoc, conversation.id, context))
                  ?.chat;
        let fold = await readBack(home, conversation, watch.value, context);
        let usage = await usageOf(home, conversation.id, context);
        if (stopped) return;
        const transcript = transcriptOf(chat, fold, usage, before);
        if (rebound) send({ type: "turn", chat, ops: [{ op: "reset" }] });
        send({
            ...transcript,
            ...(number === undefined ? {} : { number }),
            ...(to === undefined ? {} : { to }),
        });
        to = undefined;
        before = undefined;
        watch.start(async (value, updates) => {
            const ops = await fold.advance(value, context);
            const replaced = updates.some((op) => op[0] === "r");
            if (fold.stale || replaced) {
                ops.push({ op: "reset" });
                fold = await readBack(home, conversation, value, context);
            }
            const next = await usageOf(home, conversation.id, context);
            if (JSON.stringify(next) !== JSON.stringify(usage)) {
                usage = next;
                ops.push({ op: "usage", usage });
            }
            if (stopped || id !== conversation.id) return;
            const numbered = number === undefined ? {} : { number };
            if (ops.length > 0) send({ type: "turn", chat, ...numbered, ops });
            if (ops.some((op) => op.op === "reset"))
                send({ ...transcriptOf(chat, fold, usage), ...numbered });
        });
    };
    const agent = await addressed(home.harness, chat.with, context);
    if (agent === undefined)
        throw new Error(`no agent named \`${chat.with.agent}\``);
    let pointer: Awaited<ReturnType<Home["harness"]["watchDoc"]>> | undefined;
    let key: string | undefined;
    const follow = async () => {
        const agent = await addressed(home.harness, chat.with, context);
        if (!agent || stopped) return;
        const next = agentKey(agent);
        if (!chat.from && next !== key) {
            await pointer?.stop();
            pointer = await home.harness.watchDoc(ChatDoc, next, context);
            key = next;
            pointer?.start(bind);
        }
        await bind();
    };
    const office =
        !chat.from && chat.past === undefined && chat.with.agent === "mayor"
            ? await home.harness.watchDoc(RecordDoc, chat.with.hamlet, context)
            : undefined;
    try {
        await follow();
        office?.start(follow);
    } catch (error) {
        await office?.stop();
        await pointer?.stop();
        await watch?.stop();
        throw error;
    }
    return {
        ok: true,
        value: {
            stop: async () => {
                stopped = true;
                await office?.stop();
                await pointer?.stop();
                await watch?.stop();
            },
        },
    };
}

/** An older transcript page, read without changing the chat's running watch (S5). */
export async function pageChat(
    home: Home,
    chat: ChatRef,
    before: string,
    context: Context,
): Promise<ChiefResult<Transcript>> {
    const outcome = await conversationOf(home, chat, context);
    if (!outcome.ok) return outcome;
    const conversation = outcome.value;
    const watch = await conversation.watch(context);
    try {
        const fold = await readBack(home, conversation, watch.value, context);
        const usage = await usageOf(home, conversation.id, context);
        const number = chat.from
            ? undefined
            : (await home.harness.snapshot(WithDoc, conversation.id, context))
                  ?.chat;
        return {
            ok: true,
            value: {
                ...transcriptOf(chat, fold, usage, before),
                ...(number === undefined ? {} : { number }),
            },
        };
    } finally {
        await watch.stop();
    }
}

function transcriptOf(
    chat: ChatRef,
    fold: Fold,
    usage: UsageView,
    before?: string,
): Transcript {
    const end =
        before === undefined
            ? fold.items.length
            : fold.items.findIndex((item) => item.id === before);
    if (end < 0) throw new Error(`There is no item \`${before}\` here.`);
    const start = Math.max(0, end - 40);
    return {
        type: "transcript",
        chat,
        items: fold.items.slice(start, end),
        earlier: start === 0 ? null : (fold.items[start]?.id ?? null),
        ...fold.status,
        last: fold.last,
        usage,
    };
}

/** The conversation `chat` names: the current one of the chief's chat with its agent, or a past one by number. */
async function conversationOf(
    home: Home,
    chat: ChatRef,
    context: Context,
): Promise<ChiefResult<Conversation>> {
    const access = await chiefAccess(
        home,
        chat.with.hamlet,
        "open chat",
        context,
    );
    if (!access.ok) return access;
    if (chat.from) return agentConversation(home, chat, context);
    const agent = await addressed(home.harness, chat.with, context);
    if (agent === undefined) {
        throw new Error(`no agent named \`${chat.with.agent}\``);
    }
    if (chat.past === undefined) {
        return {
            ok: true,
            value: await chatOf(home, { agent, chief: home.chief }, context),
        };
    }
    const key = agentKey(agent);
    const past = (await home.harness.snapshot(ChatDoc, key, context))?.chats[
        home.chief
    ]?.conversations.find((each) => each.number === chat.past);
    const conversation =
        past && (await home.harness.conversation(past.conversation, context));
    if (!conversation) {
        throw new Error(`There is no past session \`${chat.past}\` here.`);
    }
    return { ok: true, value: conversation };
}

/** The transcript of `conversation` as of `view`, from the whole of its history. */
async function readBack(
    home: Home,
    conversation: Conversation,
    view: ConversationView,
    context: Context,
): Promise<Fold> {
    const fold = new Fold(home);
    const last = view.entries.reduce<EntryId | undefined>(
        (latest, entry) =>
            latest === undefined || entry.id > latest ? entry.id : latest,
        undefined,
    );
    let cursor: Cursor | undefined;
    while (last !== undefined) {
        const page = await conversation.entries(
            { order: "ascending", maxEntryId: last },
            PAGE,
            cursor,
            context,
        );
        for (const entry of page.items) await fold.entry(entry, context);
        if (page.next === undefined) break;
        cursor = page.next;
    }
    await fold.advance(view, context);
    return fold;
}

/** A chat's items as a client holds them, kept in step with the conversation, with the ops that keep a client in step. */
class Fold {
    readonly items: Item[] = [];
    status = { working: false, pending: 0 };
    last: LastActivity | null = null;
    /** True once an answer landed without some of what it showed being written, which no op takes away. */
    stale = false;
    #ops: Op[] = [];
    #last: EntryId | undefined;
    /** Tool items by their call's id. */
    readonly #calls = new Map<string, Item>();
    /** Items of the answer being written, by content block. */
    readonly #live = new Map<number, Item>();

    readonly #home: Home;

    constructor(home: Home) {
        this.#home = home;
    }

    /** Takes in what `view` holds beyond what was folded, and gives the ops for it. */
    async advance(view: ConversationView, context: Context): Promise<Op[]> {
        const previousLast = this.last;
        // A summary leads the active range ahead of older kept entries.
        const fresh = view.entries
            .filter(
                (entry) => this.#last === undefined || entry.id > this.#last,
            )
            .sort((a, b) => a.id - b.id);
        for (const entry of fresh) await this.entry(entry, context);
        const live = view.docs["pi.live"] as LiveState | undefined;
        const message = live?.generation?.message as
            | AssistantMessage
            | undefined;
        if (message !== undefined) this.#partial(message);
        const inbox = view.docs["pi.inbox"] as InboxState | undefined;
        const status = {
            working: live?.run !== undefined,
            pending: (inbox?.items ?? []).filter(
                (item) => item.mode !== "write",
            ).length,
        };
        if (
            status.working !== this.status.working ||
            status.pending !== this.status.pending ||
            previousLast !== this.last
        ) {
            this.status = status;
            this.#ops.push({ op: "status", ...status, last: this.last });
        }
        return this.#ops.splice(0);
    }

    /** Takes in one entry of the conversation. */
    async entry(entry: EntryRecord, context: Context): Promise<void> {
        this.#last = entry.id;
        const message = entry.model?.[0];
        if (ToolResultEntry.is(entry) && message?.role === "toolResult") {
            const item = this.#calls.get(message.toolCallId);
            if (item?.tool === undefined) return;
            const end = ending(message);
            Object.assign(item.tool, end);
            this.#ops.push({ op: "close", item: item.id, ...end });
            return;
        }
        const answer = AssistantEntry.is(entry);
        for (const [block, item] of await itemsOf(entry, this.#home, context)) {
            const live = answer ? this.#live.get(block) : undefined;
            if (answer) this.#live.delete(block);
            this.#add(item, live);
            const what =
                item.tool !== undefined
                    ? `tool ${item.tool.name}`
                    : item.kind === "assistant" &&
                        message?.role === "assistant" &&
                        message.stopReason !== "toolUse"
                      ? "replied"
                      : item.kind === "error"
                        ? "error"
                        : undefined;
            if (what !== undefined) this.last = { what, at: item.at };
            if (item.tool !== undefined && message?.role === "assistant") {
                const call = message.content[block];
                if (call?.type === "toolCall") this.#calls.set(call.id, item);
            }
        }
        if (answer) {
            this.stale ||= this.#live.size > 0;
            this.#live.clear();
            return;
        }
        // The answer being written stays last, as it is read back.
        for (const live of this.#live.values()) this.#add(live, live);
    }

    /** Takes in the answer being written. */
    #partial(message: AssistantMessage): void {
        for (const [block, item] of blocksOf(message, (i) => `live:${i}`)) {
            if (item.tool !== undefined) continue;
            // A partial credential's prefix stays held until the next committed chunk.
            item.text = new Scrubber(this.#home.credentials.secrets()).push(
                item.text,
            );
            const shown = this.#live.get(block);
            if (shown === undefined) {
                this.#live.set(block, item);
                this.#add(item, undefined);
            } else if (
                item.text.length > shown.text.length &&
                item.text.startsWith(shown.text)
            ) {
                const text = item.text.slice(shown.text.length);
                shown.text = item.text;
                this.#ops.push({ op: "append", item: shown.id, text });
            }
        }
    }

    /** Adds `item` last, in the place of `replaced` when it is given. */
    #add(item: Item, replaced: Item | undefined): void {
        if (replaced !== undefined) {
            this.items.splice(this.items.indexOf(replaced), 1);
        }
        this.items.push(item);
        this.#ops.push({
            op: "add",
            item: structuredClone(item),
            ...(replaced === undefined ? {} : { replaces: replaced.id }),
        });
    }
}

/** The items `entry` shows, by the content block each comes from. */
async function itemsOf(
    entry: EntryRecord,
    home: Home,
    context: Context,
): Promise<[number, Item][]> {
    const id = `${entry.id}:0`;
    if (ChiefEntry.is(entry)) {
        const { text, chief, at } = entry.data;
        return [[0, { id, at, kind: "user", text, chief, rewindable: true }]];
    }
    if (NoticeEntry.is(entry)) {
        const { text, at } = entry.data;
        return [[0, { id, at, kind: "notice", text }]];
    }
    if (CompactionEntry.is(entry)) {
        return [
            [
                0,
                {
                    id,
                    at: entry.model?.[0]?.timestamp ?? 0,
                    kind: "notice",
                    text: SUMMARY_NOTICE,
                },
            ],
        ];
    }
    if (ReflectionEntry.is(entry)) {
        const { at } = entry.data;
        return [[0, { id, at, kind: "notice", text: "reflection" }]];
    }
    const message = entry.model?.[0];
    if (RetryEntry.is(entry)) {
        const { at } = entry.data;
        return [[0, { id, at, kind: "notice", text: textOf(message) }]];
    }
    if (
        (LetterEntry.is(entry) ||
            QuestionEntry.is(entry) ||
            ReplyEntry.is(entry)) &&
        message?.role === "user"
    ) {
        const from = entry.data.from;
        const record = await home.harness.snapshot(
            RecordDoc,
            from.hamlet,
            context,
        );
        const agent =
            record?.agents.find((a) => a.number === from.number) ??
            record?.archive.find((a) => a.agent.number === from.number)?.agent;
        const author = agent
            ? `${agent.name}@${from.hamlet}`
            : `${from.hamlet}#${from.number}`;
        return [
            [
                0,
                {
                    id,
                    at: entry.data.at,
                    kind: "user",
                    text: textOf(message),
                    author,
                },
            ],
        ];
    }
    if (UserEntry.is(entry) && message?.role === "user") {
        const at = message.timestamp;
        return [[0, { id, at, kind: "user", text: textOf(message) }]];
    }
    if (AssistantEntry.is(entry) && message?.role === "assistant") {
        return blocksOf(message, (block) => `${entry.id}:${block}`);
    }
    return [];
}

/** The items of an answer's content blocks, and the error it ended with. */
function blocksOf(
    message: AssistantMessage,
    id: (block: number) => string,
): [number, Item][] {
    const at = message.timestamp;
    const items = message.content.flatMap((block, index): [number, Item][] => {
        if (block.type === "toolCall") {
            const summary = [...JSON.stringify(block.arguments)]
                .slice(0, SUMMARY)
                .join("");
            const tool = {
                name: block.name,
                summary,
                exit: null,
                ms: null,
                failure: null,
                change:
                    block.name === "write"
                        ? null
                        : (changeOf(block.name, block.arguments) ?? null),
            };
            return [
                [index, { id: id(index), at, kind: "tool", text: "", tool }],
            ];
        }
        const [kind, text] =
            block.type === "text"
                ? (["assistant", block.text] as const)
                : (["reasoning", block.thinking] as const);
        return text.trim() === ""
            ? []
            : [[index, { id: id(index), at, kind, text }]];
    });
    if (message.stopReason === "error" || message.stopReason === "aborted") {
        const block = message.content.length;
        const text = message.errorMessage ?? message.stopReason;
        items.push([block, { id: id(block), at, kind: "error", text }]);
    }
    return items;
}

/** The end of a tool call, from its result. */
function ending(message: Extract<Message, { role: "toolResult" }>): {
    exit: number;
    ms: number | null;
    failure: string | null;
    change?: import("./protocol.ts").Change;
} {
    const code = (message.details as { exitCode?: unknown } | null | undefined)
        ?.exitCode;
    const failure = message.isError
        ? (textOf(message)
              .replace(/<\/?harness>\n?/g, "")
              .replace(/^\[error\] /gm, "")
              .split("\n")
              .find((line) => line.trim() !== "")
              ?.replace(/^Error: /, "") ?? "")
        : null;
    return {
        exit: typeof code === "number" ? code : message.isError ? 1 : 0,
        ms: message.durationMs ?? null,
        failure,
        ...(record(message.details).change === undefined
            ? {}
            : {
                  change: record(message.details)
                      .change as import("./protocol.ts").Change,
              }),
    };
}

/** The text of a message's content. */
function textOf(message: Message | undefined): string {
    if (message === undefined || message.role === "assistant") return "";
    const { content } = message;
    if (typeof content === "string") return content;
    return content
        .flatMap((part) => (part.type === "text" ? [part.text] : []))
        .join("\n");
}

/** Kept edit previews, including the old content of a whole-file write (C30). */
export function turnTools() {
    return defineExtension({
        name: "hamlet.turns",
        hooks: [
            hook(ToolTask, {
                afterTool: async (_call, result, api, context) => {
                    const change = await api.memo<
                        import("./protocol.ts").Change
                    >(CHANGE, context);
                    return change === undefined
                        ? undefined
                        : {
                              ...result,
                              details: { ...record(result.details), change },
                          };
                },
            }),
        ],
    });
}
